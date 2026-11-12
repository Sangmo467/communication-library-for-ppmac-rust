# 密码与密钥登录

所有 SDK 都通过同一个 `pmac_connect_submit` 和 russh 认证路径，不维护第二套 SSH 实现。

## 认证方式

- 密码：配置 username + password。
- 密钥：配置 username + 私钥路径；私钥有口令时配置 passphrase。
- 公钥路径可作为配置记录传递；russh 实际从私钥推导认证公钥。
- 同时配置私钥和密码时优先使用私钥。私钥认证失败后不会自动进行密码重试；需要密码登录时请显式移除私钥配置。
- 认证被拒绝、私钥读取/解密失败返回 `PMAC_AUTHENTICATION`。

## C++

```cpp
pmac::ConnectOptions password;
password.host = "192.0.2.10";
password.username = "root";
password.password = "secret";
password.knownHosts = "C:/pmac/known_hosts";
client.connectSync(password);

pmac::ConnectOptions key;
key.host = "192.0.2.10";
key.username = "root";
key.privateKey = "C:/pmac/id_ed25519";
key.passphrase = "private-key-password";
key.knownHosts = "C:/pmac/known_hosts";
client.connectSync(key);
```

## C#

```csharp
var options = new ConnectOptions
{
    Host = "192.0.2.10",
    Username = "root",
    Password = "secret", // 或 PrivateKeyPath = "C:/pmac/id_ed25519"
    KnownHostsPath = "C:/pmac/known_hosts",
};
await client.ConnectAsync(options);
```

## Python

```python
client.connect(
    "192.0.2.10",
    "root",
    password="secret",
    known_hosts="C:/pmac/known_hosts",
)

# 密钥方式：
client.connect(
    "192.0.2.10",
    "root",
    private_key="C:/pmac/id_ed25519",
    passphrase="private-key-password",
    known_hosts="C:/pmac/known_hosts",
)
```

## C

在 `pmac_connect_options` 中设置带长度的 `username`，然后选择：

- `password`；或
- `private_key` 和可选 `passphrase`。

同时设置 `known_hosts` 或 32 字节 `pinned_sha256`。`PMAC_CONNECT_ACCEPT_ANY_HOST_KEY` 是显式风险模式，不能替代登录凭据，也不建议用于生产环境。

## 验证边界

当前自动化已验证 C ABI 对密码、私钥、公钥路径和私钥口令的无损传递；真实 SSH 密码/密钥认证、旧控制器算法交集和私钥格式兼容性仍需测试服务器及目标硬件验收。生产日志不得记录密码、私钥内容或口令。
