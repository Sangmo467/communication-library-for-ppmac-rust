# 变更记录

[中文](CHANGELOG.md) | [English](CHANGELOG.en.md)

本项目采用语义化版本。公开 C ABI 的 major/minor 独立记录；ABI major 2 内保持既有输入结构布局兼容。

## 2.0.1

- 增加 Linux ARM64/ARMv7、Windows ARM64、macOS Intel/Apple Silicon 的构建与发布流程，保留现有四平台。
- 统一平台目标目录、动态库名称及 PE/ELF/Mach-O 架构校验，增加逐目标 C/Rust ABI 对照。
- 补齐公开用户文档英文版及中英文导航，保留中文注释调用例程。
- C ABI 保持 2.1；macOS 提供独立的 x64/ARM64 dylib 包。

## 未发布

- 修复绑定层请求提取诊断丢失和执行确定性误报、自定义 Session 关闭 panic 后无法重试、取消等待后迟到 SSH Close 缓存残留；减少 Gather 行解析重复分配，CI 增加 Rust 1.100 文档表格检查。

- GetResponse 新增可选 passthrough 模式：按原文发送单条 GPASCII 命令，跳过规则表、online/program 分类及本地 open 检查；C/C++、C#、Python、MATLAB 均可设置，默认关闭且保留错误上报。

- 根据 Rust 1.100.0 新增 Cargo 清单诊断，修复五个 crate 的仓库元数据继承；补充升级审查和 Windows NASM 构建要求。

- 构建工具链及 CI 升级至 Rust 1.100.0；最低支持版本仍为 Rust 1.99。

- 补充以 GetResponse 为主的 C/C++、C#、Python 和 MATLAB 可运行示例与详细接入文档；MATLAB 新增同步 C++17 MEX 网关。Gather/geterrors 专题位于基础请求之后。

- 每个 Client 的 GPASCII 通道上限调整为 16，保留不确定释放占用；标准 error # 诊断即使随 ACK 返回也作为远端错误上报，error #31 失效旧程序上下文且不隐藏重发或补发 close。

- 新增 geterrors 的 C ABI 与 C++17 实时状态变化监控：SSH 专用 PTY、可选日期和掩码、分类行回调、有界队列以及 Ctrl+C 停止/释放屏障，详见[使用指南](docs/geterrors.md)。

- 新增实时连续 Gather 的 C ABI 与 C++17 封装，支持地址、采样周期、监听端口、有界批次回调、停止与释放屏障。
- Gather 通过控制器来源 IP 和每会话随机握手接纳反向 TCP；错误或沉默连接共用启动截止时间。数据仍是明文 TCP，部署边界详见[详细使用指南](docs/gather.md)。
- 修复复合 `close all buffers` 未失效本地程序上下文的问题。
- Python 在 ctypes 转换前校验请求及连接配置整数范围，拒绝静默截断。

## 2.0.0 - 2026-11-12

首个 Rust + russh 独立版本，C ABI 2.1。

### 新增

- 增量 GPASCII/Shell 协议处理、严格数值解析、状态位和字节正则提取。
- Runtime/Client/Request 不透明 ID，同步与异步请求、取消、deadline 和完成广播。
- 显式 SSH 连接池、严格调用线程—GPASCII Channel 亲和、维护屏障和成员级故障隔离。
- 密码与私钥认证、known_hosts 和 SHA-256 主机指纹校验。
- 持久 Shell、隔离 exec、SFTP 文件传输及程序 buffer 上传。
- C ABI、C++17/C++98、.NET 8 和 Python >=3.9 绑定。
- 电机、坐标系、PLC 常用命令与状态查询。
- MIT 许可证、第三方许可说明、构建安装文档和完整使用手册。

### 兼容性

- 使用 `pmac_` 前缀，不兼容旧 1.x API/ABI。
- GetResponse 的精确空模式、`number`、`variable` 保留旧提取语义；其他模式按字节正则执行，详见[模式说明](docs/pattern-migration.md)。
- 不自动重试已可能执行的写操作；应用需要根据执行确定性和设备状态完成恢复。
