# Communication Library for Power PPMAC

[简体中文](README.md) | **English**

A Rust/russh communication library for Power PPMAC, with a stable C ABI, C++17 and C++98 wrappers, .NET 8 and Python bindings, and a MATLAB GetResponse MEX example.

Version: **2.0.0** · C ABI: **2.1** · License: **MIT**

## Features

- GPASCII commands, synchronous/asynchronous GetResponse, response copying and regular-expression extraction.
- Calling-thread/channel affinity, up to 16 online GPASCII channels per Client, explicit connection pools and member fault isolation.
- Persistent shell sessions, isolated exec requests, SFTP upload/download and program-buffer upload.
- Common motor, coordinate-system and PLC operations and status queries.
- Continuous Gather sampling with configurable periods, addresses, listening ports and batch callbacks.
- Live geterrors monitoring through a dedicated SSH PTY, configurable masks and a stop barrier for callback lifetime management.
- Request cancellation, execution deadlines, bounded admission, completion notifications and explicit execution-certainty reporting.
- SSH host verification using known_hosts or a pinned SHA-256 fingerprint; password and private-key authentication.

GetResponse passthrough can be enabled per request to bypass the online-command rule table and program classification, while preserving framing validation. See the [passthrough guide and examples (Chinese)](docs/getresponse-passthrough.md).

## Prebuilt SDKs

Download packages from [GitHub Releases](https://github.com/Sangmo467/communication-library-for-ppmac-rust/releases). The initial release targets **Windows x86/x64** and **Linux x86/x64**. ARM and macOS packages are deferred until their build and test paths are implemented and verified.

Each SDK contains the native library, C headers, language bindings, independently buildable examples with detailed Chinese comments, CMake integration, licenses, an SPDX dependency inventory and a SHA-256 manifest. Consumers do not need Rust to use a prebuilt SDK.

Read the [English SDK setup and release guide](docs/prebuilt-sdk.en.md) for package selection, offline checks, controller access and automated publishing.

## Repository structure

| Crate | Responsibility |
| --- | --- |
| pmac-protocol | GPASCII/shell framing, numeric parsing, status bits and extraction patterns |
| pmac-core | Runtime, Client, Request, pools, cancellation and lifecycle management |
| pmac-transport-russh | SSH, GPASCII, shell and SFTP transport |
| pmac-ffi | The stable pmac_ C ABI, producing pmac.dll or libpmac.so |
| pmac-testkit | Deterministic mock transports and regression tests |

The repository includes source code, public documentation, bindings, examples, offline tests, build scripts and locked dependencies. Compiled SDKs are distributed as release assets. Internal review records and site-specific data are not part of the public source snapshot.

## Build from source

Use **Rust 1.100.0**, as pinned in rust-toolchain.toml, and the native C/C++ toolchain for your target.

```sh
cargo build -p pmac-ffi --release --locked
cargo test --workspace --locked
```

Libraries are written to target/release, or target/<triple>/release when --target is specified. CMake can build and install the native library and headers:

```sh
cmake -S . -B build/native -DCMAKE_BUILD_TYPE=Release
cmake --build build/native --config Release
cmake --install build/native --config Release --prefix /your/install/prefix
```

On Windows, select a matching Visual Studio platform (-A x64 or -A Win32). The [English SDK guide](docs/prebuilt-sdk.en.md) describes how to link an already compiled library without invoking Cargo.

## Quick start

Python 3.9 or later: install bindings/python or add it to PYTHONPATH. Set PMAC_LIBRARY to the absolute path of the DLL/so, and configure the controller address, credentials and known_hosts file outside the source code.

```python
import os
import pmac

with pmac.Runtime(library_path=os.environ["PMAC_LIBRARY"]) as runtime:
    with runtime.create_client() as client:
        client.connect(
            os.environ["PMAC_HOST"],
            os.environ.get("PMAC_USER", "root"),
            password=os.environ.get("PMAC_PASSWORD"),
            known_hosts=os.environ["PMAC_KNOWN_HOSTS"],
        )
        print(client.get_response("Sys.ServoTime").decode("ascii").strip())
```

C++17:

```cpp
#include "pmac.hpp"
#include <cstdlib>
#include <iostream>

int main() {
    pmac::Runtime runtime;
    auto client = runtime.createClient();
    pmac::ConnectOptions connection;
    connection.host = std::getenv("PMAC_HOST");
    connection.username = "root";
    connection.knownHosts = std::getenv("PMAC_KNOWN_HOSTS");
    if (const char* password = std::getenv("PMAC_PASSWORD"))
        connection.password = password;
    client.connectSync(connection);
    const auto data = client.getResponse("Sys.ServoTime");
    std::cout.write(reinterpret_cast<const char*>(data.data()), data.size());
}
```

For complete, buildable C, C++, C# and Python examples, including offline checks and error handling, see [SDK examples](docs/prebuilt-sdk.en.md). MATLAB requires a separately installed, compatible MATLAB/compiler environment.

## Documentation

The following detailed references are currently in Chinese; the repository introduction and SDK guide are available in English.

- [GetResponse examples](docs/getresponse-examples.md), [passthrough mode](docs/getresponse-passthrough.md)
- [User manual](docs/user-manual.md), [source build and installation](docs/build-and-install.md)
- [Authentication and host verification](docs/authentication.md), [C ABI contract](docs/abi-contract.md)
- [.NET](docs/dotnet-getresponse.md), [Python](docs/python-getresponse.md), [MATLAB](docs/matlab-getresponse.md)
- [DART](docs/dart-api.md), [online commands](docs/online-commands.md), [response extraction](docs/pattern-migration.md)
- [Continuous Gather](docs/gather.md), [geterrors monitoring](docs/geterrors.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md), [changelog](CHANGELOG.md)

## Operational boundaries

- CONFIRMED means the protocol acknowledgment was received; it does not mean physical motion or a program has finished.
- An operation whose acknowledgment is lost after sending may return MAY_HAVE_EXECUTED. The library does not automatically replay commands that could have side effects.
- Network Abort, Kill and JogStop commands do not replace an independent hardware safety chain.
- Verify SSH host identity in production. Do not record passwords, private-key material or passphrases in logs.
- Different calling threads or pool members do not imply read-after-write ordering or a simultaneous snapshot.
- Complete Runtime shutdown and release all child objects before unloading the native library.

## License

This project is licensed under the [MIT License](LICENSE). The modified russh 0.65.1 dependency retains its Apache-2.0 license and notices; see [third-party notices](THIRD_PARTY_NOTICES.md).
