#!/bin/sh
# 解压 LoongArch64 SDK 后运行；仅检查本机和库，不连接或操作控制器。
set -eu
export LC_ALL=C
case "$(uname -m)" in
    loongarch64|loong64) ;;
    *) echo 'This SDK requires a LoongArch64 process/host.' >&2; exit 1 ;;
esac
lib=${1:-"$(CDPATH= cd "$(dirname "$0")/.." && pwd)/native/libpmac.so"}
[ -f "$lib" ] || { echo "Missing library: $lib" >&2; exit 1; }
command -v readelf >/dev/null 2>&1 || { echo 'Install binutils (readelf) first.' >&2; exit 1; }
readelf -h "$lib" | grep -Eq 'Machine:.*LoongArch' || { echo 'Wrong ELF machine.' >&2; exit 1; }
flags=$(readelf -h "$lib" | sed -n 's/.*Flags: *\(0x[0-9a-fA-F]*\).*/\1/p')
[ -n "$flags" ] && [ "$((flags & 255))" -eq 67 ] || { echo 'LP64D/new-world ELF ABI v1 required.' >&2; exit 1; }
[ -e /lib64/ld-linux-loongarch-lp64d.so.1 ] || { echo 'New-world LoongArch dynamic loader is missing.' >&2; exit 1; }
version=$(getconf GNU_LIBC_VERSION | awk '{print $2}')
awk -v version="$version" 'BEGIN {split(version,a,".");exit !(a[1]>2 || (a[1]==2 && a[2]>=36))}' || { echo "glibc 2.36+ required; found $version" >&2; exit 1; }
grep -Eiq '(^|[[:space:]])lsx([[:space:]]|$)' /proc/cpuinfo || { echo 'LSX capability is not reported by this kernel.' >&2; exit 1; }
printf 'LoongArch64 LP64D host preflight passed; glibc %s.\n' "$version"
echo 'Next: build examples with CMake and run CTest to verify this exact OS/runtime.'
