# LoongArch64 Linux SDK：麒麟与 Debian

[中文](loongarch-sdk.md) | [English](loongarch-sdk.en.md)

目标为 `loongarch64-unknown-linux-gnu`，库为 `native/libpmac.so`。本页只讨论 LoongArch64；不包含 MIPS。龙芯型号名称不能代替 ABI 检查，调用进程、编译器和库必须使用一致的指令集及 ABI。

## 兼容条件

- 64 位、小端、LP64D 双精度硬浮点 ABI、ELF object ABI v1（新世界 ABI）。
- CPU 支持 LSX；内核提供主线 Linux 5.19 或兼容的 UAPI。厂商回移植内核不能只按版本号判断。
- 构建目标使用 glibc 2.36 基线，发布门禁检查库的实际 GLIBC 符号版本不超过 2.36。
- 麒麟、Debian 的发行版名称本身不保证兼容。须以机器上的 glibc、动态加载器、CPU 功能和离线测试结果为准；旧世界 ABI 不在本包范围内。
- 不将 glibc 拷入 SDK，也不要求替换系统 libc。构建用低版本 sysroot 只用于链接与隔离验证。

本次没有提供目标机器或具体发行版版本，因此不宣称通过麒麟或 Debian 的实机认证。QEMU 测试验证指定 ABI 与运行环境，不能代替具体发行版、控制器固件和现场网络验收。

## 在目标机器上检查并编译例程

从解压后的 SDK 根目录执行：

```sh
uname -m
cat /etc/os-release
getconf GNU_LIBC_VERSION
# 本脚本只读取平台信息和 ELF，不连接控制器；需要 binutils/readelf。
sh scripts/test-loongarch-host.sh
cmake -S examples -B ../pmac-loongarch-examples -DCMAKE_BUILD_TYPE=Release
cmake --build ../pmac-loongarch-examples --parallel 2
ctest --test-dir ../pmac-loongarch-examples --output-on-failure
```

C、C++17、C++98 例程直接引用包内已编译库，不需要安装 Rust。Python 调用需要同架构的 Python，设置 `PMAC_LIBRARY` 为 `libpmac.so` 的绝对路径；调用方式见 [Python 指南](python-getresponse.md)。.NET、MATLAB 不在本架构的自动运行验证范围内。

## 自动构建与验收

构建入口为 `scripts/Build-LoongArchSdk.ps1`，在 Linux Docker host 上调用。容器提供 GCC 15 交叉编译器、Binutils 和 QEMU；复用 runner 固定的 Rust 与 PowerShell。

GNU glibc 2.36 源码先通过固定 SHA-256 校验，再生成专用 sysroot。Rust 测试以串行方式在 QEMU 中运行；C/C++ 示例执行六项离线 CTest；C 和 Rust 探针对比 149 项大小、对齐、字段偏移。库另检查 ELF machine、LP64D、object ABI 和 GLIBC 符号版本。兼容性报告随包内 evidence 提供。

C++ 例程使用构建容器中交叉工具链匹配的 libstdc++ 运行环境。其运行通过不代表把该工具链的 libstdc++ 分发给所有系统；使用方应在目标系统以本机编译器重新编译例程。库的低版本 libc 运行验证单独执行。

参考：[Rust LoongArch Linux 要求](https://doc.rust-lang.org/rustc/platform-support/loongarch-linux.html)、[龙芯 ELF ABI](https://loongson.github.io/LoongArch-Documentation/LoongArch-ELF-ABI-EN.html)、[GNU glibc 源码](https://ftp.gnu.org/gnu/glibc/)。
