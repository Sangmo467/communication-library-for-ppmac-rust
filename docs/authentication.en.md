# Authentication and Host Identity Verification

[简体中文](authentication.md)

All SDKs use the same `pmac_connect_submit` and russh authentication path; there is no second SSH implementation.

## Login methods

- **Password**: configure username and password.
- **Private key**: configure username and the private key path; also set passphrase if the key is encrypted.
- A public key path may be passed as a configuration record; russh derives the authentication public key from the private key.
- When both a private key and password are configured, the private key takes priority. A failed private key authentication does not automatically retry with a password; explicitly remove the private key configuration to log in with a password.
- Rejected authentication, or failure to read or decrypt a private key, returns `PMAC_AUTHENTICATION`.

Read passwords, private key contents, and key passphrases from secure application configuration or the process environment. Do not include them in source code, logs, or release packages.

## Host identity

Host identity is independent of login credentials. You must choose one of the following:

1. `known_hosts`: a host-record file in OpenSSH format;
2. `pinned_sha256`: the raw 32-byte SHA-256 value of the host public key;
3. `accept_any_host_key`: explicitly accept the risk of skipping host identity verification, solely for nonproduction environments explicitly controlled by the caller.

An `@revoked` entry in `known_hosts` matching the host and key takes precedence over ordinary allow entries. Other markers and revoked host patterns containing wildcards or negation are currently unsupported; these return `PMAC_HOST_KEY`. Adjust the configuration before retrying.

The fingerprint is not a 64-character hexadecimal string; the C ABI and SDKs accept a 32-byte value. A changed host key returns `PMAC_HOST_KEY`. Verify the device identity first instead of simply disabling verification.

## C++

```cpp
pmac::ConnectOptions options;
options.host = "192.0.2.10";
options.username = "root";
options.knownHosts = "C:/pmac/known_hosts";
if (const char* value = std::getenv("PMAC_PASSWORD")) options.password = value;
client.connectSync(options);
```

For private key authentication, replace the password setting with:

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

For private key authentication, set `PrivateKeyPath` and optionally `Passphrase`, and clear `Password`.

## Python

```python
client.connect(
    "192.0.2.10",
    "root",
    password=os.environ.get("PMAC_PASSWORD"),
    known_hosts="C:/pmac/known_hosts",
)
```

For private key authentication, use `private_key="C:/pmac/id_ed25519"` and optionally `passphrase`, and pass `password=None`.

## C

Set the length-delimited `host` and `username` fields in `pmac_connect_options`, then choose:

- `password`; or
- `private_key` and optionally `passphrase`.

Also set `known_hosts` or `pinned_sha256` containing exactly 32 bytes. `PMAC_CONNECT_ACCEPT_ANY_HOST_KEY` is an explicit risk mode and does not replace login credentials.
