# 变更记录

本项目采用语义化版本。公开 C ABI 的 major/minor 独立记录；ABI major 2 内保持既有输入结构布局兼容。

## 未发布

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
