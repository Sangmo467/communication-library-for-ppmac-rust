"""Bounded read-only PMAC soak driver. Reports JSON lines to stdout; never writes files."""
import argparse
import json
import math
import os
import random
import signal
import sys
import threading
import time
from datetime import datetime, timezone


QUERIES = ("Sys.ServoCount", "Sys.PhaseTime", "Sys.ServoTime")


class Metrics:
    def __init__(self, sample_limit=4096):
        self.lock = threading.Lock()
        self.samples = []
        self.limit = sample_limit
        self.random = random.Random(0)
        self.count = self.errors = self.maximum_us = 0
        self.error_examples = []

    def success(self, latency_us):
        with self.lock:
            self.count += 1
            self.maximum_us = max(self.maximum_us, latency_us)
            if len(self.samples) < self.limit:
                self.samples.append(latency_us)
            else:
                index = self.random.randrange(self.count)
                if index < self.limit:
                    self.samples[index] = latency_us

    def failure(self, error):
        with self.lock:
            self.errors += 1
            if len(self.error_examples) < 8:
                self.error_examples.append({"type": type(error).__name__, "status": getattr(error, "status", None), "certainty": getattr(error, "certainty", None)})

    def snapshot(self):
        with self.lock:
            values = sorted(self.samples)
            quantile = lambda p: values[min(len(values) - 1, int(math.ceil(p * len(values))) - 1)] if values else None
            return {"successful_requests": self.count, "errors": self.errors, "maximum_us": self.maximum_us,
                    "latency_us": {"p50": quantile(.5), "p95": quantile(.95), "p99": quantile(.99), "p999": quantile(.999)},
                    "quantile_method": "uniform reservoir estimate", "sample_count": len(values), "sample_limit": self.limit,
                    "error_examples": list(self.error_examples)}


def validate_numeric(command, response):
    value = response.decode("ascii").strip()
    if "=" in value:
        name, value = value.split("=", 1)
        if name.strip() and name.strip().lower() != command.lower():
            raise ValueError("response name does not match submitted query")
        value = value.strip()
    if command == "Sys.ServoCount":
        if not value.isdigit():
            raise ValueError("counter response is not an unsigned decimal integer")
        int(value)
    elif not math.isfinite(float(value)):
        raise ValueError("timing response is not finite")


def resident_bytes():
    if os.name == "nt":
        import ctypes as c
        class Counters(c.Structure):
            _fields_ = [("cb", c.c_uint32), ("PageFaultCount", c.c_uint32)] + [(name, c.c_size_t) for name in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]
        counters = Counters()
        counters.cb = c.sizeof(counters)
        current = c.windll.kernel32.GetCurrentProcess
        current.restype = c.c_void_p
        get_info = c.windll.psapi.GetProcessMemoryInfo
        get_info.argtypes = [c.c_void_p, c.POINTER(Counters), c.c_uint32]
        if get_info(current(), c.byref(counters), counters.cb):
            return counters.WorkingSetSize
    else:
        try:
            with open("/proc/self/status", encoding="utf-8") as source:
                for line in source:
                    if line.startswith("VmRSS:"):
                        return int(line.split()[1]) * 1024
        except OSError:
            pass
    return None


def emit(kind, metrics, started, args, **extra):
    event = {"kind": kind, "utc": datetime.now(timezone.utc).isoformat(), "elapsed_seconds": time.monotonic() - started,
             "process_cpu_seconds": time.process_time(), "resident_bytes": resident_bytes(), "threads": args.threads,
             "pool_size": args.pool_size, "queries": QUERIES,
             "latency_clock": {"name": "perf_counter_ns", "resolution_seconds": time.get_clock_info("perf_counter").resolution, "implementation": time.get_clock_info("perf_counter").implementation},
             **metrics.snapshot(), **extra}
    print(json.dumps(event, sort_keys=True), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--duration-seconds", type=float, default=30)
    parser.add_argument("--threads", type=int, choices=(1, 2, 8, 32), default=1)
    parser.add_argument("--pool-size", type=int, default=1)
    parser.add_argument("--rate", type=float, default=100, help="aggregate request rate ceiling")
    parser.add_argument("--timeout-ms", type=int, default=5000)
    parser.add_argument("--heartbeat-seconds", type=float, default=30)
    parser.add_argument("--self-test", action="store_true", help="exercise metrics and parsers without loading the library or networking")
    args = parser.parse_args()
    if args.self_test:
        metrics = Metrics(8)
        for number in range(100):
            metrics.success(number)
        assert metrics.snapshot()["sample_count"] == 8
        assert metrics.snapshot()["maximum_us"] == 99
        for query in QUERIES:
            validate_numeric(query, b"0")
            validate_numeric(query, b"=0")
            validate_numeric(query, (query + "=0").encode("ascii"))
        try:
            validate_numeric("Sys.ServoTime", b"nan")
        except ValueError:
            pass
        else:
            raise AssertionError("nonfinite response accepted")
        try:
            validate_numeric("Sys.ServoTime", b"Sys.PhaseTime=1")
        except ValueError:
            pass
        else:
            raise AssertionError("mismatched named response accepted")
        print(json.dumps({"self_test": "passed", "network_used": False}))
        return 0
    if args.duration_seconds <= 0 or args.rate <= 0 or args.timeout_ms < 1 or not 1 <= args.pool_size <= 255 or args.heartbeat_seconds <= 0:
        parser.error("duration, rate, heartbeat and timeout must be positive; pool size must be 1..255")
    host, user = os.environ.get("PMAC_HOST"), os.environ.get("PMAC_USER", "root")
    known_hosts, pin = os.environ.get("PMAC_KNOWN_HOSTS"), os.environ.get("PMAC_PINNED_SHA256")
    if not host or not (known_hosts or pin):
        parser.error("set PMAC_HOST and PMAC_KNOWN_HOSTS or PMAC_PINNED_SHA256; arbitrary host keys are not accepted")
    try:
        pinned = bytes.fromhex(pin) if pin else None
    except ValueError:
        parser.error("PMAC_PINNED_SHA256 must be 64 hexadecimal characters")
    if pinned is not None and len(pinned) != 32:
        parser.error("PMAC_PINNED_SHA256 must be 64 hexadecimal characters")
    import pmac
    metrics = Metrics()
    stop = threading.Event()
    interrupted = threading.Event()
    def interrupt(_signum, _frame):
        interrupted.set()
        stop.set()
    signal.signal(signal.SIGINT, interrupt)
    if hasattr(signal, "SIGTERM"):
        signal.signal(signal.SIGTERM, interrupt)
    started = time.monotonic()
    connection_started = started
    with pmac.Runtime(library_path=os.environ.get("PMAC_LIBRARY")) as runtime:
        with runtime.create_client(max_requests=max(64, args.threads * 2)) as client:
            client.connect(host, user, os.environ.get("PMAC_PASSWORD"), private_key=os.environ.get("PMAC_PRIVATE_KEY"),
                           passphrase=os.environ.get("PMAC_PASSPHRASE"), known_hosts=known_hosts,
                           pinned_sha256=pinned, control_pool_size=args.pool_size)
            connection_seconds = time.monotonic() - connection_started
            started = time.monotonic()
            deadline = started + args.duration_seconds
            def worker(worker_index):
                sequence = worker_index
                while not stop.is_set() and time.monotonic() < deadline:
                    command = QUERIES[sequence % len(QUERIES)]
                    sequence += 1
                    request_start = time.monotonic()
                    request_start_ns = time.perf_counter_ns()
                    try:
                        with client.command_async(command, timeout_ms=args.timeout_ms, max_response_bytes=4096, read_pool=args.pool_size > 1) as request:
                            identity = request.id
                            info = request.wait(args.timeout_ms + 1000)
                            if info.request_id != identity:
                                raise ValueError("completed request ID differs from submitted ID")
                            validate_numeric(command, request.response())
                        metrics.success((time.perf_counter_ns() - request_start_ns) / 1000)
                    except Exception as error:
                        metrics.failure(error)
                        stop.set()
                        return
                    stop.wait(max(0, args.threads / args.rate - (time.monotonic() - request_start)))
            workers = [threading.Thread(target=worker, args=(index,), name=f"pmac-soak-{index}") for index in range(args.threads)]
            for thread in workers:
                thread.start()
            next_heartbeat = time.monotonic() + args.heartbeat_seconds
            emit("started", metrics, started, args, connection_seconds=connection_seconds, library_version=runtime.lib.pmac_version_string().decode("ascii"),
                 validation_scope="read-only numeric syntax, named-response correlation when present, and request-ID identity; no mechanical or target acceptance")
            while any(thread.is_alive() for thread in workers):
                for thread in workers:
                    thread.join(timeout=.1)
                if time.monotonic() >= next_heartbeat:
                    emit("heartbeat", metrics, started, args)
                    next_heartbeat += args.heartbeat_seconds
            emit("completed", metrics, started, args, interrupted=interrupted.is_set(), duration_requested_seconds=args.duration_seconds)
    return 130 if interrupted.is_set() else (1 if metrics.snapshot()["errors"] else 0)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(json.dumps({"kind": "failed", "type": type(error).__name__, "status": getattr(error, "status", None)}), flush=True)
        raise SystemExit(1)
