# 变更记录

[中文](CHANGELOG.md) | [English](CHANGELOG.en.md)

本项目采用语义化版本。公开 C ABI 的 major/minor 独立记录；ABI major 2 内保持既有输入结构布局兼容。

## 2.0.6

- 独立 $$$/$$$*** 即使开启透传也使用专用复位响应与维护屏障，避免无 ACK 时被当作普通命令等待。
- 从普通、透传及复位命令响应中移除 STDIN Open for ASCII Input，保留内部就绪探测。
- 修复复位后旧会话已确认关闭却残留不确定释放记录的问题；显式修复连接池后可恢复查询。
- 补充复位分包、透传和释放确认回归及超时/恢复说明；C ABI 保持 2.1。

## 2.0.5

- 修复 C#/Python 自动回收的原生资源释放、C++ Client 移动赋值失败时句柄丢失、连接池失败回滚及释放确认状态。
- 修复 Gather/geterrors 队列内存上限、远端进程清理确认、长路径上传错误分类、业务接口旧错误信息和 Shell 输出上限。
- 优化 GPASCII 分片解析、指定正则捕获提取、请求完成清理与采样缓冲分配；清理 DART 失效预算条目。
- C++ Client 移动赋值现在可抛出清理错误；C ABI 仍为 2.1。

## 2.0.4

- 修复连续 Gather 不交付数据：使用小写 gather -w，通过会话私有 FIFO 保持标准输入打开，无需 PTY；先配置采集，再启动读取进程，最后设置 Gather.Enable=3。
- 停止时清理 FIFO 和会话锁；修正原生数字版本为 20004，C ABI 保持 2.1。

## 2.0.3

- 原生版本字符串自动跟随 Cargo 版本，并增加 C 导出版本一致性测试。

- GetResponse/command 默认开启透传；显式关闭后启用 online/program 规则校验，只读池请求自动分类。新增 PMAC_REQUEST_CLASSIFY（8），C ABI 保持 2.1。
- 根据软件参考手册及 DART、Modbus 独立规范核对 66 个结构根名称，前缀匹配不区分大小写，后续成员和多维下标交给控制器判断；自定义数组使用圆括号。
- 补齐 Modbus Client/Server 在线命令、运动程序及 16 种 jog/jogret 正文原样上传测试，提供中英文结构目录及使用说明。

## 2.0.2

- 增加 LoongArch64 Linux SDK、LP64D 新世界 ABI 和 glibc 2.36 基线检查。
- 隔离交叉工具链，增加 QEMU Rust/C/C++ 运行验证、149 项 ABI 对照和目标机检查入口。
- 增加麒麟、Debian 中英文接入指南，按实际 ABI 与系统库限定兼容范围；C ABI 保持 2.1。

## 2.0.1

- 增加 Linux ARM64/ARMv7、Windows ARM64、macOS Intel/Apple Silicon 的构建与发布流程，保留现有四平台。
- 统一平台目标目录、动态库名称及 PE/ELF/Mach-O 架构校验，增加逐目标 C/Rust ABI 对照。
- 补齐公开用户文档英文版及中英文导航，保留中文注释调用例程。
- C ABI 保持 2.1；macOS 提供独立的 x64/ARM64 dylib 包。

## 未发布

- GetResponse/command 默认开启透传；显式关闭启用 online/program 规则，只读池自动分类。新增 PMAC_REQUEST_CLASSIFY（8），请求结构布局不变。
- 核对 66 个结构根名称，仅按前缀及边界匹配，不区分大小写；自定义数组使用圆括号。
- 支持 Modbus Client/Server 在线命令，补齐运动及 jog/jogret 程序缓冲区原文上传测试和中英文目录。

- 修复绑定层请求提取诊断丢失和执行确定性误报、自定义 Session 关闭 panic 后无法重试、取消等待后迟到 SSH Close 缓存残留；减少 Gather 行解析重复分配，CI 增加 Rust 1.100 文档表格检查。

- GetResponse 新增可选 passthrough 模式：按原文发送单条 GPASCII 命令，跳过规则表、online/program 分类及本地 open 检查；C/C++、C#、Python、MATLAB 均可设置，默认开启、可显式关闭且保留错误上报。

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
