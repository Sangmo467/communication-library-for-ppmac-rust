# 认证与主机身份校验

所有 SDK 都通过同一个 `pmac_connect_submit` 和 russh 认证路径，不维护第二套 SSH 实现。

## 登录方式

- **密码**：配置 username 和 password。
- **私钥**：配置 username 和私钥路径；私钥加密时另设 passphrase。
- 公钥路径可作为配置记录传递；russh 实际从私钥推导认证公钥。
- 同时配置私钥和密码时优先使用私钥。私钥认证失败后不会自动进行密码重试；需要密码登录时请显式移除私钥配置。
- 认证被拒绝、私钥读取或解密失败返回 `PMAC_AUTHENTICATION`。

密码、私钥内容和私钥口令应从应用的安全配置或进程环境读取，不写入源码、日志和发布包。

## 主机身份

主机身份与登录凭据相互独立，必须选择以下方式之一：

1. `known_hosts`：OpenSSH 格式的主机记录文件；
2. `pinned_sha256`：主机公钥的 32 字节 SHA-256 原始字节；
3. `accept_any_host_key`：显式接受不校验主机身份的风险，仅用于调用方明确控制的非生产环境。

`known_hosts` 中匹配主机和密钥的 `@revoked` 条目优先于普通允许条目。暂不支持其他 marker，以及包含通配符或否定项的撤销主机模式；遇到这些内容会返回 `PMAC_HOST_KEY`，需调整配置后重试。

指纹不是 64 字符十六进制文本；C ABI 和 SDK 接收的是 32 字节值。主机密钥变化会返回 `PMAC_HOST_KEY`，应先核实设备身份，不要直接关闭校验。

## C++

```cpp
pmac::ConnectOptions options;
options.host = "192.0.2.10";
options.username = "root";
options.knownHosts = "C:/pmac/known_hosts";
if (const char* value = std::getenv("PMAC_PASSWORD")) options.password = value;
client.connectSync(options);
```

私钥方式将密码替换为：

```cpp
options.privateKey = "C:/pmac/id_ed25519";
if (const char* value = std::getenv("PMAC_PASSPHRASE")) options.passphrase = value;
```

## C#

```csharp
var options = new ConnectOptions
{
    Host = "192.0.2.10",
    Username = "root",
    Password = Environment.GetEnvironmentVariable("PMAC_PASSWORD"),
    KnownHostsPath = "C:/pmac/known_hosts",
};
await client.ConnectAsync(options);
```

私钥方式设置 `PrivateKeyPath` 和可选 `Passphrase`，并清除 `Password`。

## Python

```python
client.connect(
    "192.0.2.10",
    "root",
    password=os.environ.get("PMAC_PASSWORD"),
    known_hosts="C:/pmac/known_hosts",
)
```

私钥方式使用 `private_key="C:/pmac/id_ed25519"` 和可选 `passphrase`，并传 `password=None`。

## C

在 `pmac_connect_options` 中设置带长度的 `host`、`username`，然后选择：

- `password`；或
- `private_key` 和可选 `passphrase`。

同时设置 `known_hosts` 或恰好 32 字节的 `pinned_sha256`。`PMAC_CONNECT_ACCEPT_ANY_HOST_KEY` 是显式风险模式，不能替代登录凭据。
