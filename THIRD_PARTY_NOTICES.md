# 第三方组件与许可证

本项目自身采用根目录的 MIT License。第三方组件仍使用各自许可证，本项目的 MIT 许可不改变第三方组件的许可条件。

## 直接随源码分发的组件

### russh 0.65.1

- 上游：https://github.com/warp-tech/russh
- crate：https://crates.io/crates/russh/0.65.1
- 许可证：Apache License 2.0
- 本地目录：`vendor/russh/`
- 许可证全文：`vendor/russh/LICENSE`

本地版本保留 russh 的版权和许可声明，并包含以下适配：

1. 将 RSA、PKCS1 依赖调整为稳定版本，并适配借用 ASN.1 类型及非弃用的 RSA 类型名称；
2. 调整 SSH Channel Close 生命周期，保留通道记录直到收到对端关闭确认；
3. 扩展 known_hosts 解析以保留撤销标记，并对不支持的安全标记和撤销模式报错。

公开源码仅保留构建所需组件，不包含上游独立示例、基准或测试程序。完整变更以该目录源码与上游 0.65.1 的差异为准。

## Cargo 依赖

其余 Rust 依赖通过 Cargo 获取，名称、版本和来源校验值锁定在 `Cargo.lock`。这些依赖的许可证由各自包提供；再分发包含这些依赖的二进制时，使用方应根据实际构建产物履行相应许可和声明义务。

常用依赖包括 Tokio、russh-sftp、regex、thiserror、zeroize、sha2 和 md5。依赖列表可能随锁文件更新，应以对应发布提交的 `Cargo.lock` 为准。
