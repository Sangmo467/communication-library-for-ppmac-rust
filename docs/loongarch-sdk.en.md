# LoongArch64 Linux SDK: Kylin and Debian

[中文](loongarch-sdk.md) | [English](loongarch-sdk.en.md)

The target is `loongarch64-unknown-linux-gnu`; the library is `native/libpmac.so`. This page covers LoongArch64 only, excluding MIPS. A processor family name does not identify its ABI: the application process, compiler and library must use matching instruction sets and ABIs.

## Compatibility requirements

- 64-bit little-endian LP64D double-precision hardware floating point, ELF object ABI v1 (new-world ABI).
- LSX-capable CPU and a kernel implementing upstream Linux 5.19 UAPI or a compatible backport. Vendor kernel version numbers alone cannot establish compatibility.
- The build targets a glibc 2.36 baseline. Publication checks that the library's actual GLIBC symbol requirements do not exceed 2.36.
- A Kylin or Debian distribution name alone does not establish compatibility. Check the installed glibc, dynamic loader, CPU features and offline tests. Old-world ABI is outside this package's scope.
- The SDK does not distribute glibc or replace the system libc. The older build sysroot is used for linking and isolated validation only.

No target hardware or exact distribution release was supplied, so no Kylin or Debian hardware certification is claimed. QEMU tests validate the specified ABI/runtime; they do not replace acceptance on a particular distribution, controller firmware or production network.

## Check the target machine and build examples

Run from the extracted SDK root:

```sh
uname -m
cat /etc/os-release
getconf GNU_LIBC_VERSION
# Reads platform and ELF metadata only; no controller connection. Requires binutils/readelf.
sh scripts/test-loongarch-host.sh
cmake -S examples -B ../pmac-loongarch-examples -DCMAKE_BUILD_TYPE=Release
cmake --build ../pmac-loongarch-examples --parallel 2
ctest --test-dir ../pmac-loongarch-examples --output-on-failure
```

C, C++17 and C++98 examples link directly against the included prebuilt library; Rust is not required. Python callers need a matching interpreter; set `PMAC_LIBRARY` to the absolute library path. See the [Python guide](python-getresponse.en.md). .NET and MATLAB execution are outside the automated validation scope for this architecture.

## Automated build and acceptance checks

Run `scripts/Build-LoongArchSdk.ps1` on a Linux Docker host. The container supplies GCC 15 cross compilers, Binutils and QEMU, while reusing the runner's pinned Rust and PowerShell installations.

GNU glibc 2.36 source is verified against a fixed SHA-256 before building a dedicated sysroot. Rust tests run serially under QEMU; C/C++ examples execute six offline CTests; C and Rust probes compare 149 size, alignment and field-offset values. Additional checks cover ELF machine, LP64D, object ABI and GLIBC symbol requirements. Compatibility evidence is included in the SDK.

C++ examples run against the cross toolchain's matching libstdc++ environment. This does not distribute that libstdc++ to every target OS: consumers should rebuild examples with their target system's compiler. The library's older-libc runtime checks execute separately.

References: [Rust LoongArch Linux requirements](https://doc.rust-lang.org/rustc/platform-support/loongarch-linux.html), [Loongson ELF ABI](https://loongson.github.io/LoongArch-Documentation/LoongArch-ELF-ABI-EN.html), [GNU glibc source](https://ftp.gnu.org/gnu/glibc/).
