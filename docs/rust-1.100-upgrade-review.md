# Rust 1.100.0 升级审查

依据 [Rust 1.100.0 官方发布说明](https://doc.rust-lang.org/stable/releases.html#version-11000-2026-11-12)与 [Cargo lint 文档](https://doc.rust-lang.org/stable/cargo/reference/lints.html)，审查当前工作区代码及构建流程。验证平台为 Windows x64 MSVC；未进行控制器连机测试或 Linux/ARM 交叉编译。

## 已落实的修改

- 固定工具链、CI 和构建文档统一为 1.100.0；保留现有最低支持版本声明 1.99。本次仅验证 1.100.0，不等于重新验证所有最低版本依赖。
- 新版 Cargo 提示 workspace.package.repository 未被任何成员继承。五个 crate 补充 repository.workspace = true，使发布元数据正确继承仓库地址，消除告警。
- 补充 Windows aws-lc-sys 的 NASM 构建要求和随依赖提供的预编译汇编对象选项。首次构建缺少 NASM 的失败属于环境依赖问题，非 Rust 源码回归。

## 新特性和兼容性检查

| 变化 | 检查与处理 |
| --- | --- |
| never type 稳定、Infallible 成为别名、fallback 变化 | 当前项目无显式 Infallible 实现冲突；全工作区编译、Clippy 和测试通过，未发现需要适配的推断错误 |
| if let 闭包捕获更精确、dyn 兼容性检查扩展 | 当前 async/trait 调用路径编译通过；连接、取消、停止屏障等已有回归测试通过 |
| 重复 repr 和 drop_in_place 新诊断 | 全目标 Clippy 无警告；未改动 C ABI 结构和导出签名 |
| Cargo 中间产物目录布局变化 | CMake 与打包脚本使用最终 DLL/so 路径；检查范围未发现依赖 deps/incremental 目录布局的解析逻辑 |
| Ord min/max 调用方式变化 | 项目源码未发现自定义 Ord/PartialOrd 实现；现有行为测试通过 |
| 自定义分配器、Arc/Box map、DropGuard | 未发现经测量可获益的替换点；分配器和释放顺序涉及 FFI、回调与线程生命周期，本次未引入相关运行时改写 |
| trim_prefix/trim_suffix、Vec 与 Cow 比较等便捷 API | 现有 strip_prefix 用法需要 Option 分支语义，不能机械替换；未发现为比较而额外分配 Vec/Cow 的生产路径 |

上述结果是定向兼容性审查，并非形式化正确性证明或性能基准；不宣称运行速度提升。后续若有 Gather 吞吐、分配次数或延迟瓶颈，应先采集基准，再评估分配器等优化。

## 验证

- rustc 1.100.0 (0ef1bc386 2026-11-09)。
- cargo fmt --all --check：通过。
- cargo clippy --workspace --all-targets --locked -- -D warnings：通过，清单告警修复后无警告。
- cargo test --workspace --locked：227 个测试通过，0 失败；文档测试通过。
- 构建环境设置 AWS_LC_SYS_PREBUILT_NASM=1，使用依赖随包提供的汇编对象。
- 原始日志位于 target/rust-1.100-clippy-final.log 和 target/rust-1.100-tests.log（忽略的本地产物）。