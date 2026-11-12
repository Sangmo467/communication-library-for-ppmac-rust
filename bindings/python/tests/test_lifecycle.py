import asyncio
import ctypes as C
import gc
import secrets
import threading
import time
import unittest
import weakref
from concurrent.futures import ThreadPoolExecutor
from unittest.mock import patch

from pmac import core
from pmac import native as N


class FakeNative:
    def __init__(self):
        self.lock = threading.RLock()
        self.changed = threading.Condition(self.lock)
        self.sequence = 0
        self.before_wait = None
        self.last_pending_id = None
        self.wait_calls = 0
        self.next_id = 10
        self.requests = {}
        self.releases = []
        self.client_releases = []
        self.runtime_releases = []
        self.runtime_release_attempts = []
        self.runtime_release_status = 0
        self.auth = None
        self.shutdown_status = 0
        self.block = False
        self.submits = 0
        self.pool_calls = []

    def pmac_runtime_create(self, config, output):
        output._obj.value = 1
        return 0

    def pmac_runtime_wake(self, runtime):
        with self.changed:
            self.sequence += 1
            self.changed.notify_all()
        return 0

    def pmac_runtime_wait_completion(self, runtime, cursor, wait_ms):
        self.wait_calls += 1
        callback = self.before_wait
        if callback is not None and self.last_pending_id is not None:
            self.before_wait = None
            callback()
        with self.changed:
            if cursor._obj.value == self.sequence:
                self.changed.wait(wait_ms / 1000)
            if cursor._obj.value == self.sequence:
                return -6
            cursor._obj.value = self.sequence
            return 0
    def pmac_client_create(self, runtime, config, output):
        output._obj.value = 2
        return 0

    def pmac_runtime_shutdown(self, runtime, wait_ms):
        if self.shutdown_status:
            return self.shutdown_status
        with self.lock:
            for request in self.requests.values():
                if request[0] == -6:
                    request[0] = -7
                self.pmac_runtime_wake(1)
        return 0

    def pmac_runtime_release(self, runtime):
        self.runtime_release_attempts.append(runtime)
        if self.runtime_release_status:
            return self.runtime_release_status
        self.runtime_releases.append(runtime)
        return 0

    def pmac_client_close(self, client, mode, wait_ms):
        return 0

    def pmac_client_release(self, client):
        self.client_releases.append(client)
        return 0

    def submit(self, output, response=b"42"):
        with self.lock:
            self.next_id += 1
            output._obj.value = self.next_id
            self.requests[self.next_id] = [-6 if self.block else 0, response]
            self.submits += 1
            self.pmac_runtime_wake(1)
        return 0

    def pmac_connect_submit(self, client, options, output):
        options = options._obj
        self.auth = {name: C.string_at(getattr(options, name).data, getattr(options, name).len)
                     for name in ("host", "username", "password", "private_key", "public_key", "passphrase", "known_hosts")}
        return self.submit(output, b"")

    def pmac_get_response_async(self, client, data, length, options, output):
        return self.submit(output, C.string_at(data, length))

    pmac_command_submit = pmac_get_response_async

    def pmac_request_wait(self, request_id, wait_ms, output):
        with self.lock:
            if request_id not in self.requests:
                return -2
            status, response = self.requests[request_id]
            output._obj.request_id = request_id
            output._obj.status = status
            output._obj.state = 4 if status != -6 else 0
            output._obj.response_len = len(response)
            if status == -6:
                self.last_pending_id = request_id
            return status

    def pmac_request_get_route(self, request_id, member_id, epoch):
        member_id._obj.value = 7
        epoch._obj.value = 9
        return 0
    def pmac_request_cancel(self, request_id):
        with self.lock:
            request = self.requests.get(request_id)
            if request is not None and request[0] == -6:
                request[0] = -7
                self.pmac_runtime_wake(1)
        return 0

    def pmac_request_release(self, request_id):
        with self.lock:
            if request_id not in self.requests:
                return -2
            self.releases.append(request_id)
            del self.requests[request_id]
        return 0

    def pmac_request_copy_response(self, request_id, output, capacity, required):
        with self.lock:
            data = self.requests[request_id][1]
        required._obj.value = len(data)
        if capacity < len(data):
            return -11
        if data:
            C.memmove(output, data, len(data))
        return 0

    pmac_request_copy_stderr = pmac_request_copy_response

    def pmac_request_copy_match(self, request_id, pattern, length, group, output, capacity, required):
        return self.pmac_request_copy_response(request_id, output, capacity, required)

    def pmac_request_copy_error(self, request_id, output, data, capacity, required):
        output._obj.request_id = request_id
        output._obj.certainty = 0
        required._obj.value = 0
        return 0

    def pmac_pool_prepare_async(self, client, connection, profile, output):
        self.pool_calls.append(("prepare", profile._obj.target_size, profile._obj.max_members))
        return self.pmac_connect_submit(client, connection, output)

    def pmac_pool_resize_async(self, client, profile, output):
        self.pool_calls.append(("resize", profile._obj.target_size, profile._obj.max_members))
        return self.submit(output)

    def pmac_pool_repair_async(self, client, member_id, options, output):
        self.pool_calls.append(("repair", member_id, options._obj.flags))
        return self.submit(output)

    def pmac_pool_drain_async(self, client, member_id, options, output):
        self.pool_calls.append(("drain", member_id, options._obj.flags))
        return self.submit(output)

    def pmac_pool_repair_maintenance_async(self, client, options, output):
        self.pool_calls.append(("maintenance", options._obj.timeout_ms))
        return self.submit(output)

    def pmac_pool_get_state(self, client, output):
        output._obj.revision = 42
        output._obj.desired = 2
        output._obj.ready = 1
        output._obj.release_uncertain = 1
        return 0

    def pmac_pool_get_members(self, client, members, capacity, required):
        required._obj.value = 2
        if capacity < 2:
            return -11
        for index in range(2):
            members[index].member_id = index + 1
            members[index].epoch = index + 3
            members[index].state = index
            members[index].control = index == 0
        return 0

    def pmac_pool_confirm_recovery(self, client, revision, flags):
        self.pool_calls.append(("confirm", revision, flags))
        return 0
    def complete(self, request_id):
        with self.lock:
            self.requests[request_id][0] = 0
            self.pmac_runtime_wake(1)


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        self.lib = FakeNative()
        self.loader = patch.object(N, "load", return_value=self.lib)
        self.loader.start()
        self.addCleanup(self.loader.stop)
        self.runtime = core.Runtime()
        self.addCleanup(self.runtime.shutdown)
        self.client = self.runtime.create_client()

    def test_password_and_encrypted_key_options_are_copied(self):
        token = secrets.token_hex(16)
        with self.client.connect_async("example.invalid", "test", token,
                                       private_key="test-key", public_key="test-pub",
                                       passphrase=token, known_hosts="test-hosts") as request:
            request.wait()
        self.assertEqual(self.lib.auth["password"], token.encode())
        self.assertEqual(self.lib.auth["passphrase"], token.encode())
        self.assertEqual(self.lib.auth["private_key"], b"test-key")

    def test_get_response_retrieval_never_resubmits(self):
        with self.client.get_response_async("#1p") as request:
            request.wait()
            self.assertEqual(request.response(), b"#1p")
            self.assertEqual(request.response(), b"#1p")
            self.assertEqual(request.match(".*"), b"#1p")
            self.assertEqual(request.route, core.RequestRoute(7, 9))
        self.assertEqual(self.lib.submits, 1)
        self.assertEqual(len(self.lib.releases), 1)

    def test_idle_pump_keeps_first_request_and_multiple_waiters(self):
        async def run():
            with self.client.get_response_async("#1p") as request:
                first, second = await asyncio.wait_for(asyncio.gather(request.wait_async(), request.wait_async()), 1)
                self.assertEqual(first.request_id, request.id)
                self.assertEqual(second.request_id, request.id)
        asyncio.run(run())

    def test_wait_budget_does_not_cancel(self):
        self.lib.block = True
        with self.client.get_response_async("#1p") as request:
            with self.assertRaises(TimeoutError):
                request.wait(0)
            self.assertEqual(self.lib.requests[request.id][0], -6)
            self.lib.complete(request.id)
            request.wait()

    def test_async_cancellation_and_release_preserve_completion(self):
        self.lib.block = True
        async def run():
            request = self.client.get_response_async("#1p")
            request_id = request.id
            task = asyncio.create_task(request.wait_async())
            while not request._tracked:
                await asyncio.sleep(0)
            request.release()
            with self.assertRaises(core.PmacError) as caught:
                await asyncio.wait_for(task, 1)
            self.assertEqual(caught.exception.status, -7)
            self.assertEqual(caught.exception.request_id, request_id)
            self.assertEqual(self.lib.releases.count(request_id), 1)
        asyncio.run(run())

    def test_async_task_cancel_cancels_native_request(self):
        self.lib.block = True
        async def run():
            with self.client.get_response_async("#1p") as request:
                task = asyncio.create_task(request.wait_async())
                while not request._tracked:
                    await asyncio.sleep(0)
                task.cancel()
                with self.assertRaises(asyncio.CancelledError):
                    await task
                self.assertEqual(self.lib.requests[request.id][0], -7)
        asyncio.run(run())

    def test_completion_capacity_counts_pending_not_just_queue(self):
        runtime = core.Runtime(completion_capacity=1)
        self.addCleanup(runtime.shutdown)
        client = runtime.create_client()
        self.lib.block = True
        async def run():
            first = client.get_response_async("#1p")
            second = client.get_response_async("#2p")
            first_task = asyncio.create_task(first.wait_async())
            while not first._tracked:
                await asyncio.sleep(0)
            with self.assertRaises(core.PmacError) as caught:
                await second.wait_async()
            self.assertEqual(caught.exception.status, -12)
            self.lib.complete(first.id)
            await asyncio.wait_for(first_task, 1)
            self.lib.complete(second.id)
            await asyncio.wait_for(second.wait_async(), 1)
            first.release()
            second.release()
        asyncio.run(run())

    def test_parallel_requests_keep_output_and_release_ownership(self):
        def call(index):
            text = f"#{index % 32 + 1}p"
            with self.client.get_response_async(text) as request:
                request.wait()
                self.assertEqual(request.response(), text.encode())
        with ThreadPoolExecutor(max_workers=8) as pool:
            list(pool.map(call, range(512)))
        self.assertEqual(len(self.lib.releases), 512)
        self.assertEqual(len(set(self.lib.releases)), 512)

    def test_pending_shutdown_retains_id_for_retry(self):
        self.lib.shutdown_status = -14
        with self.assertRaises(core.PmacError):
            self.runtime.shutdown()
        self.assertEqual(self.runtime._id.value, 1)
        self.assertEqual(self.lib.runtime_releases, [])
        self.lib.shutdown_status = 0
        self.runtime.shutdown()
        self.runtime.shutdown()
        self.assertEqual(self.lib.runtime_releases, [1])
        self.assertEqual(self.lib.client_releases, [2])
        self.assertFalse(self.runtime._pump.thread.is_alive())

    def test_shutdown_success_but_release_pending_retains_runtime_for_retry(self):
        request = self.client.get_response_async("#1p")
        request.wait()
        self.lib.runtime_release_status = -14
        with self.assertRaises(core.PmacError) as caught:
            self.runtime.shutdown()
        self.assertEqual(caught.exception.status, -14)
        self.assertEqual(self.runtime._id.value, 1)
        self.assertTrue(self.runtime._closing)
        self.assertEqual(self.lib.runtime_releases, [])
        self.assertFalse(self.runtime._pump.thread.is_alive())
        request.release()
        self.lib.runtime_release_status = 0
        self.runtime.shutdown()
        self.assertEqual(self.lib.runtime_release_attempts, [1, 1])
        self.assertEqual(self.lib.runtime_releases, [1])
    def test_idle_runtime_and_dropped_request_are_collectable(self):
        runtime = core.Runtime()
        client = runtime.create_client()
        request = client.get_response_async("#1p")
        request_id = request.id
        request_ref = weakref.ref(request)
        del request
        gc.collect()
        self.assertIsNone(request_ref())
        self.assertEqual(self.lib.releases.count(request_id), 1)
        client.release()
        del client
        runtime_ref = weakref.ref(runtime)
        pump = runtime._pump
        del runtime
        gc.collect()
        pump.thread.join(1)
        self.assertIsNone(runtime_ref())
        self.assertFalse(pump.thread.is_alive())

    def test_completion_between_poll_and_native_wait_is_not_lost(self):
        self.lib.block = True
        self.assertTrue(self.runtime._pump.native_notifications)
        async def run():
            with self.client.get_response_async("#1p") as request:
                observed = threading.Event()
                def complete_between_poll_and_wait():
                    self.assertEqual(self.lib.last_pending_id, request.id)
                    self.lib.complete(request.id)
                    observed.set()
                self.lib.before_wait = complete_between_poll_and_wait
                result = await asyncio.wait_for(request.wait_async(), 1)
                self.assertEqual(result.request_id, request.id)
                self.assertTrue(observed.is_set())
                self.assertLess(self.lib.wait_calls, 20)
        asyncio.run(run())

    def test_two_runtime_completion_cursors_do_not_consume_each_other(self):
        other_runtime = core.Runtime()
        self.addCleanup(other_runtime.shutdown)
        other_client = other_runtime.create_client()
        self.lib.block = True
        async def run():
            with self.client.get_response_async("#1p") as first, other_client.get_response_async("#2p") as second:
                first_task = asyncio.create_task(first.wait_async())
                second_task = asyncio.create_task(second.wait_async())
                while not first._tracked or not second._tracked:
                    await asyncio.sleep(0)
                self.lib.complete(first.id)
                self.lib.complete(second.id)
                results = await asyncio.wait_for(asyncio.gather(first_task, second_task), 1)
                self.assertEqual({result.request_id for result in results}, {first.id, second.id})
        asyncio.run(run())
    def test_pool_abi_layout_and_read_pool_flag(self):
        self.assertEqual(C.sizeof(N.PoolProfile), 24)
        self.assertEqual(C.sizeof(N.PoolState), 64)
        self.assertEqual(C.sizeof(N.PoolMember), 48)
        self.assertEqual(N.PoolState.revision.offset, 8)
        self.assertEqual(N.PoolMember.last_valid_response_unix_ms.offset, 32)
        self.assertEqual(core.options({"infinite": True, "read_pool": True}).flags, 3)

    def test_pool_management_uses_request_lifecycle_and_snapshots(self):
        token = secrets.token_hex(8)
        with self.client.pool_prepare_async("example.invalid", "test", token,
                                            target_size=2, max_members=4,
                                            private_key="key-path") as request:
            request.wait()
        with self.client.pool_resize_async(3, max_members=4) as request:
            request.wait()
        with self.client.pool_repair_async(7, timeout_ms=30000) as request:
            request.wait()
        with self.client.pool_drain_async(0) as request:
            request.wait()
        with self.client.pool_repair_maintenance_async(timeout_ms=30000) as request:
            request.wait()
        state = self.client.pool_state()
        self.assertEqual((state.revision, state.desired, state.ready, state.release_uncertain), (42, 2, 1, 1))
        members = self.client.pool_members()
        self.assertEqual([member.member_id for member in members], [1, 2])
        self.assertEqual([member.state for member in members], [0, 1])
        self.client.pool_confirm_recovery(state.revision, True, True)
        self.assertEqual(self.lib.pool_calls, [("prepare", 2, 4), ("resize", 3, 4),
                                             ("repair", 7, 0), ("drain", 0, 0),
                                             ("maintenance", 30000), ("confirm", 42, 3)])
        self.assertEqual(len(self.lib.releases), 5)
        self.assertEqual(self.lib.auth["password"], token.encode())
    def test_fork_check_covers_request_and_release(self):
        with self.client.get_response_async("#1p") as request:
            with patch.object(core.os, "getpid", return_value=-1):
                with self.assertRaises(core.PmacError):
                    request.response()
                with self.assertRaises(core.PmacError):
                    request.release()
                with self.assertRaises(core.PmacError):
                    self.runtime.shutdown()

    def test_forked_finalizers_do_not_touch_inherited_native_handles(self):
        lib = FakeNative()
        with patch.object(N, "load", return_value=lib):
            runtime = core.Runtime()
        client = runtime.create_client()
        request = client.get_response_async("#1p")
        pump = runtime._pump
        with patch.object(core.os, "getpid", return_value=-1):
            del request
            del client
            del runtime
            gc.collect()
            self.assertEqual(lib.releases, [])
            self.assertEqual(lib.client_releases, [])
            self.assertEqual(lib.runtime_release_attempts, [])
        pump.stop()
        pump.thread.join(1)
        self.assertFalse(pump.thread.is_alive())


@unittest.skipUnless(hasattr(core.os, "fork") and core.os.environ.get("PMAC_LIBRARY"),
                     "requires POSIX fork and PMAC_LIBRARY")
class NativeForkTests(unittest.TestCase):
    def test_child_gc_skips_inherited_handles_and_parent_can_release(self):
        runtime = core.Runtime(library_path=core.os.environ["PMAC_LIBRARY"])
        self.addCleanup(runtime.shutdown)
        client = runtime.create_client()
        self.addCleanup(client.release)
        self.addCleanup(client.close)
        import socket
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        self.addCleanup(listener.close)
        request = client.connect_async("127.0.0.1", "test", "test",
                                       port=listener.getsockname()[1],
                                       accept_any_host_key=True, tcp_probe=False)
        self.addCleanup(request.release)
        pid = core.os.fork()
        if pid == 0:
            del request, client, runtime
            gc.collect()
            core.os._exit(0)
        deadline = time.monotonic() + 5
        while True:
            waited, status = core.os.waitpid(pid, core.os.WNOHANG)
            if waited == pid:
                break
            if time.monotonic() >= deadline:
                import signal
                core.os.kill(pid, signal.SIGKILL)
                core.os.waitpid(pid, 0)
                self.fail("child finalization deadlocked after fork")
            time.sleep(0.01)
        self.assertTrue(core.os.WIFEXITED(status))
        self.assertEqual(core.os.WEXITSTATUS(status), 0)


if __name__ == "__main__":
    unittest.main()
