# 修复后复审：4cd443c 工作区

日期：2026-11-13。基准提交 4cd443c347a913fcd16344498fb5ff8ace45a5af，加当时尚未提交的工作区修复；具体结论对应本次测试时的文件内容。复审未修改生产代码。

结论：确认 8 项运行时/兼容性问题（2 项 P1、6 项 P2），另有一项 CI 格式门禁失败。上轮 `@revoked` 同密钥、Python 电机编号、C++98 command 异常释放、PoolDrain 等待阶段回滚及 vendor LICENSE 文件计数问题已看到修复和针对性验证；上轮远端关闭配额和小响应分包问题仍有边界残留。以下问题不全由本轮修改引入，明确标出残留或原风险。

## 1. P1：普通 known_hosts 记录忽略否定匹配

位置：[known_hosts.rs:112](../vendor/russh/src/keys/known_hosts.rs#L112)、[known_hosts.rs:129](../vendor/russh/src/keys/known_hosts.rs#L129)。仅 revoked 行的否定模式被拒绝；普通行中的 `host,!host` 匹配到正项时立刻返回 true，忽略排除项。

本地真实 SSH 测试以 `[127.0.0.1]:PORT,![127.0.0.1]:PORT` 为唯一普通条目，结果连接成功，而该行应排除该主机。可使本不应信任的主机密钥被接受。OpenSSH [格式规范](https://man.openbsd.org/sshd.8#SSH_KNOWN_HOSTS_FILE_FORMAT)要求否定模式覆盖同一行的正匹配。建议完整实现匹配与否定优先，或对不支持的模式拒绝加载。上轮普通条目加 @revoked 的复现已拒绝连接。

## 2. P1：异步等待的 last_error 错误报告 NotSent

位置：[lib.rs:582](../crates/pmac-ffi/src/lib.rs#L582)、[lib.rs:606](../crates/pmac-ffi/src/lib.rs#L606)、[pmac_cpp98.hpp:186](../bindings/cpp/pmac_cpp98.hpp#L186)。`pmac_request_wait` 将 `info.code` 封装为默认 NotSent 的 Error，新的线程本地错误记录未沿用真实 info.certainty 和 request ID。C++98 getResponse 随即读取它构造异常。

受控请求已发送后超时，实测 `info.certainty=1`（MayHaveExecuted）、`last.certainty=0`、`last.id=0`。如果 raw 命令具有副作用，上层可能据错误异常认为未发送。此项是新 last_error 在异步等待上的回归；上轮同步业务方法的具体测试已修复。建议从同一请求快照填充完整错误信息，并确保 release 不在复制详情前清空它。

## 3. P2：已确认关闭仍累计单 Client 的不确定成员

位置：[client.rs:1055](../crates/pmac-core/src/client.rs#L1055)、[client.rs:1067](../crates/pmac-core/src/client.rs#L1067)、[client.rs:822](../crates/pmac-core/src/client.rs#L822)。远端 Close 已被记录且共享 GPASCII permit 可释放，但 fail_member 忽略 retire_control 结果，仍无条件将成员放入 uncertain，修复时又把它计入池限额。

本地 SSH fixture 中同一 Client 连续四次远端确认关闭，每次活动通道 0，`release_uncertain` 却增长到 4，第四次 PoolRepair 得到 ResourceLimit。上轮全局 permit 泄漏已部分解决，单 Client 池恢复仍受影响。建议按照已确认结果维护不确定记录，并测试同一 Client 重复故障/恢复。

## 4. P2：PoolDrain 已启动关闭后不再遵守 deadline，取消后仍改变成员

位置：[client.rs:990](../crates/pmac-core/src/client.rs#L990)、[client.rs:1023](../crates/pmac-core/src/client.rs#L1023)、[client.rs:1027](../crates/pmac-core/src/client.rs#L1027)。等待 Busy 的阶段受 drive 控制，但随后的 session.close 位于 drive 外，请求状态仍是 Queued。

受控 Session gate：100ms deadline 后等待到 250ms 仍未终止，关闭放行后返回 Ok/Confirmed；另一实验中关闭期间 cancel 返回 Cancelled/NotSent，随后成员仍从 Retiring 变为 Drained。真实 russh 的单次关闭有局部时限，但本测试没有使用真实设备。建议明确关闭开始后的状态与确定性，限制请求 deadline，并使后台清理与已发布终态的语义一致。

## 5. P2：Python 池成员 ID 仍可发生整数截断

位置：[core.py:601](../bindings/python/pmac/core.py#L601)、[client.rs:678](../crates/pmac-core/src/client.rs#L678)。`pool_drain_async(2**64)` 经 ctypes.c_uint64 变成 0，而 0 被解释为排空所有非控制成员。实测 CFUNCTYPE 转换收到 member_id=0。电机与 DART 参数已增加检查，池管理参数仍须在进入 ctypes 前检查范围；也应检查同类 pool_repair 的编号。

## 6. P2：单 LF 与 ACK 分包仍会被误报超长

位置：[gpascii.rs:61](../crates/pmac-protocol/src/gpascii.rs#L61)。新 framing allowance 处理 CRLF 和单 CR，未处理单 LF。预算 3、无回显响应为 `abc\n\x06` 时整包返回 abc，而单独先收到 `abc\n` 即报 ResponseTooLarge(3)，传输层可因此退役 SSH。已用当前 decoder 直接复现；原回显前缀分包用例已通过。建议覆盖 LF/CR/CRLF、ACK 之前每个拆包位置和小预算组合。

## 7. P2：DART 独立 TCP 建连超时仍退役健康 SSH

位置：[dart.rs:72](../crates/pmac-transport-russh/src/dart.rs#L72)、[client.rs:538](../crates/pmac-core/src/client.rs#L538)。已完成 GPASCII 发现后，DART 连接独立 TCP 端口仍沿用 DartConnect 错误处理；TCP 建连超时会 fatal 并触发 fail_member。

本地 SSH fixture 查询 Dart.Enable/Port 后，填满 DART TCP backlog，得到 `ExecTimeout, MayHaveExecuted, fatal=true; ssh connected=false, failed=1`。这是上轮只列出的风险，现已复现。修复时应区分服务发现期间的 GPASCII 故障与后续 TCP 故障。

## 8. P2：新版 C++ 头文件强制引用新导出，无法链接部分同为 ABI 2.1 的旧库

位置：[pmac.hpp:393](../bindings/cpp/pmac.hpp#L393)、[pmac_cpp98.hpp:32](../bindings/cpp/pmac_cpp98.hpp#L32)、[pmac.h:20](../include/pmac.h#L20)。通用 check 无条件引用新导出 pmac_copy_last_error，ABI 仍是 2001。

本机旧 DLL 返回 ABI 2001 且无该导出，用当前头文件编译仅调用既有 abort 的程序，链接失败 `undefined reference to __imp_pmac_copy_last_error`。旧 DLL 在忽略目录，无法证明来自哪个具体提交；只证明此同版本旧库组合不兼容，旧程序搭配新库的方向未测。建议新增接口升级可区分的 ABI minor/能力标志，明确最低库版本，或动态探测并保守回退。

## CI 格式门禁

当前 `cargo fmt --all --check` 返回 1，输出多个文件格式差异；[CI workflow](../.github/workflows/ci.yml#L23)运行此检查。此问题表述为当前工作区检查失败，不断言全部格式差异由本次修复引入。

## 验证记录

- `cargo test --workspace --locked --offline --quiet`：167 项通过（11+16+47+37+56）。
- Python 3.9：21 项中 20 通过，1 项 POSIX fork 用例在 Windows 跳过；.NET 项目 `dotnet build --no-restore` 通过；`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。
- `scripts/Test-DependencyPatch.ps1` 通过，记录 91 文件；`cargo fmt --all --check` 失败，见上文。
- 定向故障实验使用忽略目录中的临时 harness；生产源码未更改。FFI harness 的 lib.rs SHA-256 与当前生产文件一致；本地 SSH fixture 直接引用当前传输源码。预期正确行为的失败断言是本次缺陷证据，不是仓库正式测试失败。

```powershell
cargo test --manifest-path target/review-transport/Cargo.toml --offline --target-dir target review_known_hosts_negation -- --nocapture
cargo test --manifest-path target/ffi-rereview/Cargo.toml --offline --target-dir target review_wait_last_error -- --nocapture
cargo test --manifest-path target/review-transport/Cargo.toml --offline --target-dir target review_confirmed_close_still_exhausts_same_pool -- --nocapture
cargo test --manifest-path target/core-rereview-gate/Cargo.toml --offline --target-dir target rereview_drain_cancel_after_close_started -- --nocapture
cargo test --manifest-path target/core-rereview-gate/Cargo.toml --offline --target-dir target rereview_drain_success_after_execution_deadline -- --nocapture
$env:PYTHONPATH = Join-Path $PWD 'bindings/python'
py -3.9 target/rereview-bindings-narrow.py
./target/rereview-protocol/lf.exe
cargo test --manifest-path target/review-transport/Cargo.toml --offline --target-dir target review_dart_tcp_connect_timeout -- --nocapture
```

上轮 DART Set 对超长输入先复制后校验的内存风险仍在 `crates/pmac-ffi/src/dart.rs:39`，本轮未做大内存实验，继续列为风险，不计入确认项。未执行真实控制器运动、物理断网、跨平台运行或持续压力测试。GitNexus 图基于提交版本，未提交更改以实际源码、差异及复现为准；图空结果未当成安全结论。
