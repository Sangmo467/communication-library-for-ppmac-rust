import asyncio
import ctypes as C
from enum import IntEnum
from collections import namedtuple
import os
import queue
import threading
import time
import weakref

from . import native as N
from .status import CoordStatus, MotorStatus


RequestRoute = namedtuple("RequestRoute", "member_id epoch")

class PoolMemberState(IntEnum):
    READY = 0
    BUSY = 1
    FAILED = 2
    RETIRING = 3
    RELEASE_UNCERTAIN = 4
    CREATING = 5
    DRAINED = 6

class PmacError(Exception):
    def __init__(self, status, message, request_id=0, certainty=0):
        super().__init__(message)
        self.status = status
        self.request_id = request_id
        self.certainty = certainty


def check(status, operation="PMAC operation"):
    if status != 0:
        raise PmacError(status, f"{operation} failed with status {status}")


def _check_business(lib, status, operation):
    if status == 0:
        return
    function = getattr(lib, "pmac_copy_last_error", None)
    if function is not None:
        info, required = N.ErrorInfo(), C.c_size_t()
        copied = function(N.byref(info), None, 0, N.byref(required))
        if copied in (0, -11) and info.status == status:
            data = (C.c_uint8 * required.value)() if required.value else None
            if data is None or function(N.byref(info), data, len(data), N.byref(required)) == 0:
                message = bytes(data).decode("utf-8", "replace") if data else operation
                raise PmacError(status, message, info.request_id, info.certainty)
    raise PmacError(status, f"{operation} failed with status {status}", certainty=1)

def _checked_integer(value, minimum, maximum, name):
    if not isinstance(value, int):
        raise TypeError(f"{name} must be an integer")
    if not minimum <= value <= maximum:
        raise ValueError(f"{name} is outside the native integer range")
    return value

def options(value=None):
    value = value or {}
    return N.RequestOptions(N.sizeof(N.RequestOptions), 2000,
                            value.get("timeout_ms", 5000),
                            (1 if value.get("infinite") else 0) | (2 if value.get("read_pool") else 0),
                            value.get("max_response_bytes", 1024 * 1024))


def _release_request(lib, request_id, created_pid):
    if os.getpid() != created_pid:
        return
    lib.pmac_request_cancel(request_id)
    lib.pmac_request_release(request_id)


class Request:
    def __init__(self, runtime, request_id):
        self.runtime = runtime
        self._id = request_id
        self._lock = threading.RLock()
        self._disposed = False
        self._tracked = False
        self._completed = False
        self._result = None
        self._error = None
        self._waiters = []
        self._finalizer = weakref.finalize(self, _release_request, runtime.lib, request_id, os.getpid())

    @property
    def id(self):
        self.runtime._check_pid()
        with self._lock:
            if self._disposed or not self._id:
                raise PmacError(-2, "request released")
            return self._id

    @property
    def route(self):
        with self._lock:
            function = getattr(self.runtime.lib, "pmac_request_get_route", None)
            if function is None:
                raise PmacError(-25, "the loaded library does not provide request routing")
            member_id, epoch = C.c_uint64(), C.c_uint64()
            check(function(self.id, N.byref(member_id), N.byref(epoch)), "request route")
            return RequestRoute(member_id.value, epoch.value)
    def wait(self, wait_ms=0xFFFFFFFF):
        request_id = self.id
        info = N.RequestInfo()
        status = self.runtime.lib.pmac_request_wait(request_id, wait_ms, N.byref(info))
        if status == -6:
            raise TimeoutError("request wait budget expired")
        with self._lock:
            error = self._error_locked(status) if status else None
            self._finish_locked(info, error)
        if error:
            raise error
        return info

    async def wait_async(self):
        loop = asyncio.get_running_loop()
        future = loop.create_future()
        with self._lock:
            self.id
            if self._completed:
                self._publish(future, self._result, self._error)
            else:
                if not self._tracked:
                    self.runtime._enqueue(self)
                    self._tracked = True
                self._waiters.append(future)
        try:
            return await future
        except asyncio.CancelledError:
            try:
                self.cancel()
            except PmacError:
                pass
            raise

    def cancel(self):
        check(self.runtime.lib.pmac_request_cancel(self.id), "cancel")

    def response(self):
        return self._copy(False)

    def stderr(self):
        return self._copy(True)

    def match(self, pattern, group=0):
        view, buffer = N.view(pattern)
        with self._lock:
            request_id = self.id
            required = C.c_size_t()
            status = self.runtime.lib.pmac_request_copy_match(request_id, view.data, view.len, group, None, 0, N.byref(required))
            if status not in (0, -11):
                raise self._error_locked(status)
            data = (C.c_uint8 * required.value)() if required.value else None
            if data:
                check(self.runtime.lib.pmac_request_copy_match(request_id, view.data, view.len, group, data, len(data), N.byref(required)), "copy match")
            return bytes(data) if data else b""

    def error(self, status):
        with self._lock:
            return self._error_locked(status)

    def _error_locked(self, status):
        info = N.ErrorInfo()
        required = C.c_size_t()
        code = self.runtime.lib.pmac_request_copy_error(self._id, N.byref(info), None, 0, N.byref(required))
        if code not in (0, -11):
            return PmacError(status, f"request failed with status {status}", self._id)
        data = (C.c_uint8 * required.value)() if required.value else None
        if data:
            self.runtime.lib.pmac_request_copy_error(self._id, N.byref(info), data, len(data), N.byref(required))
        message = bytes(data).decode("utf-8", "replace") if data else f"request failed with status {status}"
        return PmacError(status, message, info.request_id, info.certainty)

    def _copy(self, stderr):
        with self._lock:
            request_id = self.id
            required = C.c_size_t()
            function = self.runtime.lib.pmac_request_copy_stderr if stderr else self.runtime.lib.pmac_request_copy_response
            status = function(request_id, None, 0, N.byref(required))
            if status not in (0, -11):
                raise self._error_locked(status)
            data = (C.c_uint8 * required.value)() if required.value else None
            if data:
                check(function(request_id, data, len(data), N.byref(required)), "copy response")
            return bytes(data) if data else b""

    @staticmethod
    def _publish(future, result, error):
        if future.done():
            return
        if error:
            future.set_exception(error)
        else:
            future.set_result(result)

    def _finish_locked(self, info, error):
        if self._completed:
            return
        self._completed, self._result, self._error = True, info, error
        waiters, self._waiters = self._waiters, []
        for future in waiters:
            try:
                future.get_loop().call_soon_threadsafe(self._publish, future, info, error)
            except RuntimeError:
                pass  # The originating event loop has already closed.
        if self._disposed:
            self._release_locked()

    def _poll(self):
        with self._lock:
            if self._completed:
                return True
            info = N.RequestInfo()
            status = self.runtime.lib.pmac_request_wait(self._id, 0, N.byref(info))
            if status == -6:
                return False
            self._finish_locked(info, self._error_locked(status) if status else None)
            return True

    def _release_locked(self):
        if self._id:
            check(self.runtime.lib.pmac_request_release(self._id), "request release")
            self._id = 0
            self._finalizer.detach()

    def release(self):
        self.runtime._check_pid()
        with self._lock:
            if self._disposed:
                return
            self._disposed = True
            self.runtime.lib.pmac_request_cancel(self._id)
            if not self._tracked or self._completed:
                self._release_locked()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.release()


class _CompletionPump:
    def __init__(self, lib, runtime_id, capacity):
        if capacity < 1:
            raise ValueError("completion_capacity must be positive")
        self.lib, self.runtime_id = lib, runtime_id
        self.native_notifications = hasattr(lib, "pmac_runtime_wait_completion") and hasattr(lib, "pmac_runtime_wake")
        self.queue = queue.Queue(maxsize=capacity)
        self.slots = threading.BoundedSemaphore(capacity)
        self.wake = threading.Event()
        self.stopping = threading.Event()
        self.thread = threading.Thread(target=self.run, name="pmac-completion", daemon=True)
        self.thread.start()

    def _wake(self):
        if self.native_notifications:
            self.lib.pmac_runtime_wake(self.runtime_id)
        else:
            self.wake.set()

    def enqueue(self, request):
        if self.stopping.is_set():
            raise PmacError(-13, "runtime is closing")
        if not self.slots.acquire(blocking=False):
            raise PmacError(-12, "completion waiter capacity is full; request remains active", request.id)
        try:
            self.queue.put_nowait(weakref.ref(request))
        except BaseException:
            self.slots.release()
            raise
        self._wake()

    def run(self):
        pending = []
        cursor = C.c_uint64()
        try:
            while True:
                while True:
                    try:
                        pending.append(self.queue.get_nowait())
                    except queue.Empty:
                        break
                for reference in pending[:]:
                    request = reference()
                    if request is None:
                        pending.remove(reference)
                        self.slots.release()
                        continue
                    try:
                        complete = request._poll()
                    except Exception as error:
                        with request._lock:
                            request._finish_locked(None, error)
                        complete = True
                    if complete:
                        pending.remove(reference)
                        self.slots.release()
                    request = None
                if self.stopping.is_set() and not pending and self.queue.empty():
                    return
                if self.native_notifications:
                    status = self.lib.pmac_runtime_wait_completion(self.runtime_id, N.byref(cursor), 100)
                    if status not in (0, -6) and not self.stopping.is_set():
                        raise PmacError(status, "runtime completion notification failed", certainty=1)
                else:
                    self.wake.wait(0.001 if pending else None)
                    self.wake.clear()
        except Exception as error:
            self.stopping.set()
            while True:
                try:
                    pending.append(self.queue.get_nowait())
                except queue.Empty:
                    break
            for reference in pending:
                request = reference()
                if request is not None:
                    with request._lock:
                        self.lib.pmac_request_cancel(request._id)
                        request._finish_locked(None, PmacError(-26, f"completion pump failed: {error}", request._id, 1))
                self.slots.release()
                request = None

    def stop(self):
        if self.stopping.is_set():
            return
        self.stopping.set()
        self._wake()

def _finalize_runtime(lib, runtime_id, pump, created_pid):
    if os.getpid() != created_pid:
        return
    pump.stop()
    status = lib.pmac_runtime_shutdown(runtime_id, 0)
    if status == 0:
        lib.pmac_runtime_release(runtime_id)


class Runtime:
    def __init__(self, workers=2, library_path=None, completion_capacity=1024):
        if completion_capacity < 1:
            raise ValueError("completion_capacity must be positive")
        self.lib = N.load(library_path)
        self._pid = os.getpid()
        self._lock = threading.RLock()
        self._closing = False
        self._clients = weakref.WeakSet()
        config = N.RuntimeConfig(N.sizeof(N.RuntimeConfig), 2000, workers, 0)
        self._id = C.c_uint64()
        check(self.lib.pmac_runtime_create(N.byref(config), N.byref(self._id)), "runtime create")
        try:
            self._pump = _CompletionPump(self.lib, self._id.value, completion_capacity)
        except BaseException:
            self.lib.pmac_runtime_shutdown(self._id.value, 0)
            self.lib.pmac_runtime_release(self._id.value)
            raise
        self._finalizer = weakref.finalize(self, _finalize_runtime, self.lib, self._id.value, self._pump, self._pid)

    @property
    def native_version(self):
        return self.lib.pmac_version_string().decode("utf-8")

    @property
    def native_version_number(self):
        return self.lib.pmac_version_number()

    @property
    def native_abi_version(self):
        return self.lib.pmac_abi_version()

    def info(self):
        self._check_pid()
        result = N.RuntimeInfo()
        check(self.lib.pmac_get_runtime_info(N.byref(result)), "runtime info")
        return result
    def create_client(self, max_requests=64, max_retained_bytes=80 * 1024 * 1024, max_file_tasks=1):
        with self._lock:
            self._check_open()
            config = N.ClientConfig(N.sizeof(N.ClientConfig), 2000, max_requests, max_file_tasks, max_retained_bytes)
            client_id = C.c_uint64()
            check(self.lib.pmac_client_create(self._id.value, N.byref(config), N.byref(client_id)), "client create")
            client = Client(self, client_id.value)
            self._clients.add(client)
            return client

    def _enqueue(self, request):
        with self._lock:
            self._check_open()
            self._pump.enqueue(request)

    def _check_pid(self):
        if os.getpid() != self._pid:
            raise PmacError(-2, "inherited runtime cannot be used after fork")

    def _check_open(self):
        self._check_pid()
        if not self._id.value:
            raise PmacError(-2, "runtime released")
        if self._closing:
            raise PmacError(-13, "runtime is closing")

    def shutdown(self, wait_ms=1000):
        self._check_pid()
        deadline = time.monotonic() + max(0, wait_ms) / 1000
        with self._lock:
            if not self._id.value:
                return
            self._closing = True
            status = self.lib.pmac_runtime_shutdown(self._id.value, wait_ms)
            check(status, "runtime shutdown")  # Keep the ID for a later retry on SHUTDOWN_PENDING.
            self._pump.stop()
            self._pump.thread.join(timeout=max(0, deadline - time.monotonic()))
            if self._pump.thread.is_alive():
                raise PmacError(-14, "runtime completion pump is still stopping")
            for client in list(self._clients):
                client.release()
            check(self.lib.pmac_runtime_release(self._id.value), "runtime release")
            self._id.value = 0
            self._finalizer.detach()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.shutdown()

def _finalize_client(lib, client_id, created_pid):
    if os.getpid() != created_pid:
        return
    lib.pmac_client_close(client_id, 0, 0)
    lib.pmac_client_release(client_id)


class Client:
    def __init__(self, runtime, client_id):
        self.runtime, self._id, self._pid = runtime, client_id, os.getpid()
        self._lock = threading.RLock()
        self._finalizer = weakref.finalize(self, _finalize_client, runtime.lib, client_id, self._pid)
    @property
    def lib(self): return self.runtime.lib
    def _check(self):
        self.runtime._check_pid()
        with self._lock:
            if not self._id:
                raise PmacError(-2, "client released")
            return self._id

    def _connect_options(self, host, username, password, kwargs):
        native = N.ConnectOptions()
        native.struct_size, native.abi_version = N.sizeof(native), 2000
        keepalive = []
        for name, value in (("host", host), ("username", username), ("password", password), ("private_key", kwargs.get("private_key")), ("public_key", kwargs.get("public_key")), ("passphrase", kwargs.get("passphrase")), ("known_hosts", kwargs.get("known_hosts")), ("pinned_sha256", kwargs.get("pinned_sha256"))):
            view, buffer = N.view(value); setattr(native, name, view); keepalive.append(buffer)
        native.port = kwargs.get("port", 22); native.control_pool_size = kwargs.get("control_pool_size", 1)
        native.flags = (1 if kwargs.get("tcp_probe", True) else 0) | (2 if kwargs.get("accept_any_host_key", False) else 0)
        native.tcp_probe_timeout_ms = kwargs.get("tcp_probe_timeout_ms", 600); native.keepalive_interval_ms = kwargs.get("keepalive_interval_ms", 1000); native.keepalive_max = kwargs.get("keepalive_max", 3)
        return native, keepalive

    def connect_async(self, host, username, password=None, **kwargs):
        native, keepalive = self._connect_options(host, username, password, kwargs)
        request = C.c_uint64()
        check(self.lib.pmac_connect_submit(self._check(), N.byref(native), N.byref(request)), "connect submit")
        return Request(self.runtime, request.value)
    def connect(self, *args, **kwargs):
        request = self.connect_async(*args, **kwargs)
        with request: request.wait()

    def command_async(self, command, **kwargs):
        view, buffer = N.view(command); native = options(kwargs); request = C.c_uint64()
        check(self.lib.pmac_command_submit(self._check(), view.data, view.len, N.byref(native), N.byref(request)), "command submit")
        return Request(self.runtime, request.value)

    def command(self, command, **kwargs):
        with self.command_async(command, **kwargs) as request:
            request.wait(); return request.response()

    def get_response_async(self, command, **kwargs):
        view, buffer = N.view(command); native = options(kwargs); request = C.c_uint64()
        check(self.lib.pmac_get_response_async(self._check(), view.data, view.len, N.byref(native), N.byref(request)), "get response submit")
        return Request(self.runtime, request.value)

    def get_response(self, command, pattern=None, **kwargs):
        with self.get_response_async(command, **kwargs) as request:
            request.wait()
            return request.response() if not pattern else request.match(pattern)

    def dart_connect_async(self, **kwargs):
        native = options(kwargs); request = C.c_uint64()
        check(self.lib.pmac_dart_connect_async(self._check(), N.byref(native), N.byref(request)), "DART connect submit")
        return Request(self.runtime, request.value)

    def dart_connect(self, **kwargs):
        with self.dart_connect_async(**kwargs) as request: request.wait()

    def dart_disconnect_async(self, **kwargs):
        native = options(kwargs); request = C.c_uint64()
        check(self.lib.pmac_dart_disconnect_async(self._check(), N.byref(native), N.byref(request)), "DART disconnect submit")
        return Request(self.runtime, request.value)

    def dart_disconnect(self, **kwargs):
        with self.dart_disconnect_async(**kwargs) as request: request.wait()

    def dart_get_async(self, command, address, length, **kwargs):
        _checked_integer(command, 0, 0xffffffff, "command"); _checked_integer(address, 0, 0xffffffff, "address")
        _checked_integer(length, 0, 524288, "length")
        native = options(kwargs); request = C.c_uint64()
        check(self.lib.pmac_dart_get_async(self._check(), command, address, length, N.byref(native), N.byref(request)), "DART get submit")
        return Request(self.runtime, request.value)

    def dart_get(self, command, address, length, **kwargs):
        with self.dart_get_async(command, address, length, **kwargs) as request:
            request.wait(); return request.response()

    def dart_set_async(self, command, address, data, **kwargs):
        _checked_integer(command, 0, 0xffffffff, "command"); _checked_integer(address, 0, 0xffffffff, "address")
        view, buffer = N.view(data); native = options(kwargs); request = C.c_uint64()
        check(self.lib.pmac_dart_set_async(self._check(), command, address, view.data, view.len, N.byref(native), N.byref(request)), "DART set submit")
        return Request(self.runtime, request.value)

    def dart_set(self, command, address, data, **kwargs):
        with self.dart_set_async(command, address, data, **kwargs) as request: request.wait()

    def shell_async(self, command, persistent=True, **kwargs):
        view, buffer = N.view(command); native = options(kwargs); request = C.c_uint64()
        check(self.lib.pmac_shell_submit(self._check(), view.data, view.len, int(persistent), N.byref(native), N.byref(request)), "shell submit")
        return Request(self.runtime, request.value)

    def shell(self, command, persistent=True, **kwargs):
        with self.shell_async(command, persistent, **kwargs) as request:
            request.wait(); return request.response()

    def _two_paths(self, function, first, second, cleanup=None, **kwargs):
        a, ab = N.view(first); b, bb = N.view(second); native = options(kwargs); request = C.c_uint64()
        args = (self._check(), a.data, a.len, b.data, b.len) + ((int(cleanup),) if cleanup is not None else ()) + (N.byref(native), N.byref(request))
        check(function(*args), "file submit")
        with Request(self.runtime, request.value) as request_object: request_object.wait()

    def upload(self, local, remote, **kwargs): self._two_paths(self.lib.pmac_upload_submit, local, remote, **kwargs)
    def download(self, remote, local, **kwargs): self._two_paths(self.lib.pmac_download_submit, remote, local, **kwargs)
    def upload_buffer(self, local, remote, cleanup_remote=False, **kwargs): self._two_paths(self.lib.pmac_buffer_upload_submit, local, remote, cleanup_remote, **kwargs)
    def file_exists(self, remote, **kwargs):
        view, buffer = N.view(remote); native = options(kwargs); request_id = C.c_uint64()
        check(self.lib.pmac_file_exists_submit(self._check(), view.data, view.len, N.byref(native), N.byref(request_id)), "file exists submit")
        with Request(self.runtime, request_id.value) as request: request.wait(); return request.response() == b"1"
    def remove_file(self, remote, **kwargs):
        view, buffer = N.view(remote); native = options(kwargs); request_id = C.c_uint64()
        check(self.lib.pmac_file_remove_submit(self._check(), view.data, view.len, N.byref(native), N.byref(request_id)), "file remove submit")
        with Request(self.runtime, request_id.value) as request: request.wait()

    def state(self):
        result = N.StateSnapshot(); check(self.lib.pmac_client_get_state(self._check(), N.byref(result)), "state"); return result
    @property
    def connected(self): return self.state().state == 4
    def close(self, wait_ms=1000):
        with self._lock:
            if self._id:
                check(self.lib.pmac_client_close(self._check(), 0, wait_ms), "client close")
    def release(self):
        self.runtime._check_pid()
        with self._lock:
            if self._id:
                check(self.lib.pmac_client_release(self._id), "client release")
                self._id = 0
                self._finalizer.detach()
    def __enter__(self): return self
    def __exit__(self, *args):
        self.close()
        self.release()

    def _pool_function(self, name):
        function = getattr(self.lib, name, None)
        if function is None:
            raise PmacError(-25, "the loaded library does not provide pool management")
        return function

    @staticmethod
    def _pool_profile(target_size=1, max_members=None, min_ready=1, connect_timeout_ms=60000):
        return N.PoolProfile(N.sizeof(N.PoolProfile), 2000, target_size,
                             target_size if max_members is None else max_members,
                             min_ready, connect_timeout_ms)

    def pool_prepare_async(self, host, username, password=None, **kwargs):
        connection, keepalive = self._connect_options(host, username, password, kwargs)
        profile = self._pool_profile(kwargs.get("target_size", kwargs.get("control_pool_size", 1)),
                                     kwargs.get("max_members"), kwargs.get("min_ready", 1),
                                     kwargs.get("connect_timeout_ms", 60000))
        request_id = C.c_uint64()
        check(self._pool_function("pmac_pool_prepare_async")(self._check(), N.byref(connection), N.byref(profile), N.byref(request_id)), "pool prepare submit")
        return Request(self.runtime, request_id.value)

    def pool_resize_async(self, target_size, max_members=None, min_ready=1, connect_timeout_ms=60000):
        profile = self._pool_profile(target_size, max_members, min_ready, connect_timeout_ms)
        request_id = C.c_uint64()
        check(self._pool_function("pmac_pool_resize_async")(self._check(), N.byref(profile), N.byref(request_id)), "pool resize submit")
        return Request(self.runtime, request_id.value)

    def _pool_member_async(self, name, member_id, kwargs):
        _checked_integer(member_id, 0, 0xffffffffffffffff, "member_id")
        native = options(kwargs)
        request_id = C.c_uint64()
        check(self._pool_function(name)(self._check(), member_id, N.byref(native), N.byref(request_id)), "pool member management submit")
        return Request(self.runtime, request_id.value)

    def pool_repair_async(self, member_id=0, **kwargs):
        return self._pool_member_async("pmac_pool_repair_async", member_id, kwargs)

    def pool_drain_async(self, member_id=0, **kwargs):
        return self._pool_member_async("pmac_pool_drain_async", member_id, kwargs)

    def pool_repair_maintenance_async(self, **kwargs):
        native = options(kwargs)
        request_id = C.c_uint64()
        check(self._pool_function("pmac_pool_repair_maintenance_async")(self._check(), N.byref(native), N.byref(request_id)), "maintenance repair submit")
        return Request(self.runtime, request_id.value)

    def pool_state(self):
        state = N.PoolState()
        check(self._pool_function("pmac_pool_get_state")(self._check(), N.byref(state)), "pool state")
        return state

    def pool_members(self):
        function = self._pool_function("pmac_pool_get_members")
        required = C.c_size_t()
        status = function(self._check(), None, 0, N.byref(required))
        for _ in range(4):
            if status not in (0, -11):
                check(status, "pool members")
            members = (N.PoolMember * required.value)()
            status = function(self._check(), members, len(members), N.byref(required))
            if status == -11:
                continue
            check(status, "pool members")
            return list(members)[:required.value]
        raise PmacError(-12, "pool membership changed repeatedly while copying the snapshot")

    def pool_confirm_recovery(self, expected_revision, acknowledge_unknown_writes=False, confirm_remote_released=False):
        flags = (1 if acknowledge_unknown_writes else 0) | (2 if confirm_remote_released else 0)
        check(self._pool_function("pmac_pool_confirm_recovery")(self._check(), expected_revision, flags), "confirm recovery")
    def _unit(self, name, *args, **kwargs):
        if args:
            _checked_integer(args[0], -0x80000000, 0x7fffffff, "target")
        if name == "coord_exec_prog":
            _checked_integer(args[1], -0x80000000, 0x7fffffff, "program")
        native = options(kwargs); _check_business(self.lib, getattr(self.lib, "pmac_" + name)(self._check(), *args, N.byref(native)), name)
    def abort(self, **kwargs): self._unit("abort", **kwargs)
    def coord_enable(self, n, **kwargs): self._unit("coord_enable", n, **kwargs)
    def coord_exec_prog(self, n, p, **kwargs): self._unit("coord_exec_prog", n, p, **kwargs)
    def coord_hold(self, n, **kwargs): self._unit("coord_hold", n, **kwargs)
    def coord_run(self, n, **kwargs): self._unit("coord_run", n, **kwargs)
    def plc_enable(self, n, **kwargs): self._unit("plc_enable", n, **kwargs)
    def plc_disable(self, n, **kwargs): self._unit("plc_disable", n, **kwargs)
    def motor_kill(self, n, **kwargs): self._unit("motor_kill", n, **kwargs)
    def motor_enable(self, n, **kwargs): self._unit("motor_enable", n, **kwargs)
    def motor_jog_stop(self, n, **kwargs): self._unit("motor_jog_stop", n, **kwargs)
    def motor_abs_jog(self, n, value, **kwargs): self._unit("motor_abs_jog", n, float(value), **kwargs)
    def motor_inc_jog(self, n, value, **kwargs): self._unit("motor_inc_jog", n, float(value), **kwargs)

    def _query(self, name, kind, target=None, **kwargs):
        if target is not None: _checked_integer(target, -0x80000000, 0x7fffffff, "target")
        native = options(kwargs); value = kind(); args = (self._check(),) + (() if target is None else (target,)) + (N.byref(native), N.byref(value))
        _check_business(self.lib, getattr(self.lib, "pmac_" + name)(*args), name); return value.value
    def coord_status(self, n, **kwargs): return CoordStatus(self._query("coord_status", C.c_uint64, n, **kwargs))
    def motor_status(self, n, **kwargs): return MotorStatus(self._query("motor_status", C.c_uint64, n, **kwargs))
    def motor_position(self, n, **kwargs): return self._query("motor_position", C.c_double, n, **kwargs)
    def motor_desired_position(self, n, **kwargs): return self._query("motor_desired_position", C.c_double, n, **kwargs)
    def motor_velocity(self, n, **kwargs): return self._query("motor_velocity", C.c_double, n, **kwargs)
    def motor_position_error(self, n, **kwargs): return self._query("motor_position_error", C.c_double, n, **kwargs)
    def servo_error_count(self, **kwargs): return self._query("servo_error_count", C.c_uint32, **kwargs)
    def phase_error_count(self, **kwargs): return self._query("phase_error_count", C.c_uint32, **kwargs)
    def servo_count(self, **kwargs): return self._query("servo_count", C.c_uint32, **kwargs)
    def phase_time(self, **kwargs): return self._query("phase_time", C.c_double, **kwargs)
    def servo_time(self, **kwargs): return self._query("servo_time", C.c_double, **kwargs)
    def abort_all_flag(self, **kwargs): return bool(self._query("abort_all_flag", C.c_uint32, **kwargs))
    def plc_running(self, n, **kwargs): return bool(self._query("plc_running", C.c_uint32, n, **kwargs))
    def coord_prog_running(self, n, **kwargs): return bool(self._query("coord_prog_running", C.c_uint32, n, **kwargs))
    def motor_closed_loop(self, n, **kwargs): return bool(self._query("motor_closed_loop", C.c_uint32, n, **kwargs))
    def coord_motor_enabled(self, n, **kwargs): return bool(self._query("coord_motor_enabled", C.c_uint32, n, **kwargs))
