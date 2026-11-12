# 本地依赖说明

`russh/` 来自 russh 0.65.1，原包地址：

https://crates.io/api/v1/crates/russh/0.65.1/download

原包 SHA-256：

`94be3d86c71b9ed220a675c405e35c7a02565a009bf8da2e9cc13a700320a5c5`

本地修改用于采用稳定 RSA/PKCS1 依赖、适配 ASN.1 借用类型，并修正 Channel Close 的远端确认生命周期。修改后的文件保留原版权和 Apache-2.0 声明；许可证见 `russh/LICENSE`，汇总见根目录 `THIRD_PARTY_NOTICES.md`。

公开目录不包含上游独立示例、基准和测试程序。
