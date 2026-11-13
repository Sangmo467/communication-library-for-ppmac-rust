#!/usr/bin/env python3
"""Run one GetResponse request; configuration comes from PMAC_* variables."""

import argparse
import os
from pathlib import Path
import sys


def timeout_ms(value):
    parsed = int(value)
    if not 1 <= parsed <= 0xFFFFFFFF:
        raise argparse.ArgumentTypeError("timeout must be between 1 and 4294967295 ms")
    return parsed


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--command", default="Sys.ServoTime", help="controller command (default: Sys.ServoTime)")
    parser.add_argument("--pattern", default=None, help="number, variable, or byte regex; default: complete response")
    parser.add_argument("--timeout-ms", type=timeout_ms, default=5000)
    parser.add_argument("--raw", action="store_true", help="write exact returned bytes, without adding a newline")
    parser.add_argument("--native-version", action="store_true", help="load native library and print version/ABI without connecting")
    return parser.parse_args()


def required_env(name):
    value = os.environ.get(name)
    if not value:
        raise ValueError("Set " + name)
    return value


def report_pmac_error(error):
    print(
        f"PMAC failure: status={error.status}, certainty={error.certainty}, request_id={error.request_id}",
        file=sys.stderr,
    )
    # Keep the entire original diagnostic, including controller 'error #' lines.
    message = str(error)
    sys.stderr.write(message)
    if not message.endswith("\n"):
        sys.stderr.write("\n")


def main():
    args = arguments()  # --help needs neither the binding nor a native library.
    try:
        from pmac import PmacError, Runtime
    except ImportError as error:
        print(f"Cannot import pmac: {error}. Install ./bindings/python or set PYTHONPATH.", file=sys.stderr)
        return 2

    try:
        library = str(Path(required_env("PMAC_LIBRARY")).expanduser().resolve(strict=True))
        if args.native_version:
            with Runtime(library_path=library) as runtime:
                print(f"native={runtime.native_version}, ABI={runtime.native_abi_version}")
            return 0

        host = required_env("PMAC_HOST")
        known_hosts = str(Path(required_env("PMAC_KNOWN_HOSTS")).expanduser().resolve(strict=True))
        password = required_env("PMAC_PASSWORD")
        username = os.environ.get("PMAC_USER", "root")
        with Runtime(library_path=library) as runtime:
            with runtime.create_client() as client:
                # Report request errors before closing resources, so cleanup cannot hide them.
                try:
                    client.connect(host, username, password, known_hosts=known_hosts)
                    data = client.get_response(
                        args.command, pattern=args.pattern, timeout_ms=args.timeout_ms
                    )
                    if args.raw:
                        sys.stdout.buffer.write(data)
                        sys.stdout.buffer.flush()
                    else:
                        output = data.decode("utf-8", errors="replace")
                        sys.stdout.write(output)
                        if not output.endswith("\n"):
                            sys.stdout.write("\n")
                except PmacError as error:
                    report_pmac_error(error)
                    return 1
        return 0
    except PmacError as error:
        report_pmac_error(error)
        return 1
    except (OSError, ValueError, TypeError) as error:
        print(f"Configuration or local I/O failure: {error}", file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("Interrupted; the request is not replayed.", file=sys.stderr)
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
