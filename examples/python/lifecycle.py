"""Native SDK lifecycle smoke; no controller or network connection is used."""
import asyncio
import gc
import os
import time
import weakref

import pmac


def expected_failure(function):
    deadline = time.monotonic() + 1
    while True:
        try:
            function()
        except pmac.PmacError as error:
            if error.status == -3 and time.monotonic() < deadline:
                time.sleep(0)  # Retry only an admission rejection, never an accepted operation.
                continue
            assert error.status in (-7, -8, -13, -23), error.status
            return
        raise AssertionError("unconnected request unexpectedly succeeded")

def seed_failed_member(client):
    # The absent key fails local preflight before a socket is opened.
    import tempfile
    import uuid
    key_path = os.path.join(tempfile.gettempdir(), uuid.uuid4().hex)
    with client.connect_async("127.0.0.1", "test", private_key=key_path,
                              accept_any_host_key=True) as request:
        try:
            request.wait()
        except pmac.PmacError as error:
            assert error.status == -16, error.status
        else:
            raise AssertionError("missing private key unexpectedly loaded")

with pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY")) as runtime:
    with runtime.create_client(max_requests=1) as client:
        assert runtime.lib.pmac_abi_version() >= 2001
        assert client.pool_state().ready == 0 and client.pool_members() == []
        seed_failed_member(client)
        assert client.pool_state().failed == 1 and len(client.pool_members()) == 1
        for _ in range(128):
            expected_failure(lambda: client.command("#1p"))
            expected_failure(lambda: client.get_response("#1p"))
            expected_failure(lambda: client.shell("pwd"))
            expected_failure(lambda: client.upload("absent", "/tmp/pmac-test"))
            expected_failure(lambda: client.download("/tmp/pmac-test", "absent"))
            expected_failure(lambda: client.upload_buffer("absent", "/tmp/pmac-test"))


async def run():
    with pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY")) as runtime:
        with runtime.create_client() as client:
            seed_failed_member(client)
            async def one(index):
                try:
                    request = client.get_response_async(f"#{index % 8 + 1}p")
                except pmac.PmacError as error:
                    assert error.status == -23, error.status
                    return
                with request:
                    request_id = request.id
                    try:
                        await request.wait_async()
                    except pmac.PmacError as error:
                        assert error.status in (-7, -8, -13, -23), error.status
                        assert error.request_id == request_id
                        assert request.route.member_id >= 0
                    else:
                        raise AssertionError("unconnected async request unexpectedly succeeded")
            await asyncio.wait_for(asyncio.gather(*(one(index) for index in range(32))), 5)


asyncio.run(run())


def management_failure(request, expected):
    with request:
        try:
            request.wait()
        except pmac.PmacError as error:
            assert error.status == expected, error.status
        else:
            raise AssertionError("management operation unexpectedly succeeded")
        assert request.route.member_id >= 0


with pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY")) as runtime:
    with runtime.create_client() as client:
        import tempfile
        import uuid
        key_path = os.path.join(tempfile.gettempdir(), uuid.uuid4().hex)
        management_failure(client.pool_prepare_async(
            "127.0.0.1", "test", private_key=key_path, accept_any_host_key=True,
            target_size=2, max_members=4), -16)
        state = client.pool_state()
        assert (state.desired, state.ready, state.failed) == (2, 0, 1)
        try:
            client.pool_confirm_recovery(state.revision + 1, confirm_remote_released=True)
        except pmac.PmacError as error:
            assert error.status == -29
        else:
            raise AssertionError("stale pool revision was accepted")
        client.pool_confirm_recovery(state.revision, confirm_remote_released=True)
        assert client.pool_state().release_uncertain == 0
        management_failure(client.pool_repair_async(1), -16)
        management_failure(client.pool_resize_async(1, max_members=4), -23)
        management_failure(client.pool_drain_async(1), -1)
        management_failure(client.pool_repair_maintenance_async(), -23)

with pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY")) as runtime:
    assert runtime._pump.native_notifications, "latest native completion notifications are required"
    assert runtime.info().abi_major == 2
    import ctypes as C
    first, second = C.c_uint64(), C.c_uint64()
    initial = runtime.lib.pmac_runtime_wait_completion(runtime._id.value, C.byref(first), 0)
    assert initial in (0, -6)
    second.value = first.value
    assert runtime.lib.pmac_runtime_wake(runtime._id.value) == 0
    assert runtime.lib.pmac_runtime_wait_completion(runtime._id.value, C.byref(first), 0) == 0
    assert runtime.lib.pmac_runtime_wait_completion(runtime._id.value, C.byref(second), 0) == 0
    assert first.value == second.value
    assert runtime.lib.pmac_runtime_wait_completion(runtime._id.value, C.byref(first), 0) == -6
    client = runtime.create_client()
    request = client.connect_async("127.0.0.1", "test", private_key=os.path.join(
        tempfile.gettempdir(), uuid.uuid4().hex), accept_any_host_key=True)
    try:
        request.wait()
    except pmac.PmacError as error:
        assert error.status == -16
    try:
        runtime.shutdown()
    except pmac.PmacError as error:
        assert error.status == -14, error.status
    else:
        raise AssertionError("runtime released while a child request ID was still alive")
    assert request.route.member_id >= 0
    request.release()
    runtime.shutdown()
    client.release()

def dropped_runtime():
    runtime = pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY"))
    return weakref.ref(runtime), runtime._pump


reference, pump = dropped_runtime()
gc.collect()
pump.thread.join(1)
assert reference() is None
assert not pump.thread.is_alive()
print("SDK lifecycle smoke passed (768 sync failures, 32 async submissions, pool management, completion notifications, retryable release, GC)")
