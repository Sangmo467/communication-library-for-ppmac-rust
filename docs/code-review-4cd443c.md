# 4cd443c 全仓代码审核

审核日期：2026-11-13。基准：`4cd443c347a913fcd16344498fb5ff8ace45a5af`。本轮只审核，未修改生产源码，也未自动提交修复。

结论：确认 **8 项缺陷（2 项 P1、6 项 P2）**，另列 3 项尚未动态验证的风险。前一轮的工作仅为提交前检查，不能作为全仓审核通过的结论。现有测试通过不覆盖本文的故障场景。

## 范围与方法

- 覆盖第一方生产模块：pmac-core、pmac-protocol、pmac-transport-russh、pmac-ffi；C 公开头文件；C++17/C++98、Python、.NET 绑定。
- 检查请求状态/取消/执行确定性、线程亲和、连接池管理、资源回收、GPASCII/Shell/reset/DART/SFTP、认证、ABI、异常和对象生命周期；对照现有测试与文档。
- 检查 CMake、CI、依赖补丁校验及发布打包相关脚本。vendor 重点检查本地补丁、Channel Close 和 known_hosts；没有逐行重新审核上游全部密码学实现。
- 使用 GitNexus query/context 辅助定位。索引刷新最初因注册表权限和中断恢复失败，随后成功重建，meta.json 的 lastCommit 已为上述基准（3553 节点、9659 边、312 流程）。MCP 仍有旧 staleness 缓存；图构建也提示跨语言解析缺口和流程截断。因此结论均落到当前源码和复现实验，不以空调用集合证明安全。
- 动态复现均在本地受控环境进行；临时程序保存在忽略的 target 下，没有向真实控制器发送运动、复位或写入命令。

## F1 / P1：known_hosts 忽略 @revoked，已撤销主机密钥仍被接受

位置：[connection.rs:272](../crates/pmac-transport-russh/src/connection.rs#L272)、[connection.rs:121](../crates/pmac-transport-russh/src/connection.rs#L121)、[known_hosts.rs:93](../vendor/russh/src/keys/known_hosts.rs#L93)。

触发条件：known_hosts 同时存在同一 host/key 的普通允许条目，以及 `@revoked host key` 撤销条目。此场景常见于追加撤销记录而保留旧信任记录。

根因：解析器按空格取前三列，将 marker 当成 host，忽略撤销行；上层仅保留普通公钥列表，并以任一公钥匹配即接受。没有撤销优先判定。

影响：已被用户明确撤销的主机密钥仍能通过身份校验。若撤销原因是私钥泄露，撤销保护失效。本轮复现的是信任判定错误，不是实际攻击演练。

证据：使用本地真实 SSH fixture，生成普通条目及同 key 的 @revoked 条目，连接实际返回 Ok；期待 HostKey 拒绝的测试失败。OpenSSH 官方规范明确要求撤销密钥不得再被接受，见 [SSH_KNOWN_HOSTS FILE FORMAT](https://man.openbsd.org/sshd.8#SSH_KNOWN_HOSTS_FILE_FORMAT)。

建议：解析 marker，保存撤销集合并先检查撤销，再判断允许列表；加入“允许与撤销并存”的回归。不能支持的安全相关 marker 应明确报错，不能静默当作无关行。

## F2 / P1：同步业务接口丢失执行确定性，异常错误地标为 NOT_SENT

位置：[helpers.rs:65](../crates/pmac-ffi/src/helpers.rs#L65)、[lib.rs:31](../crates/pmac-ffi/src/lib.rs#L31)、[core.py:34](../bindings/python/pmac/core.py#L34)、[core.py:618](../bindings/python/pmac/core.py#L618)、[Runtime.cs:113](../bindings/dotnet/Pmac/Runtime.cs#L113)、[Models.cs:9](../bindings/dotnet/Pmac/Models.cs#L9)、[pmac.hpp:390](../bindings/cpp/pmac.hpp#L390)。

触发条件：通过同步业务方法（例如 motor_inc_jog / MotorIncJog / motorIncJog）发出有副作用命令后，超时或失去确认。

根因：核心会生成 MayHaveExecuted；business::run 经 finish_request 取回 Error 后删除原生 Request ID，ffi_status 只返回错误码。高级绑定再用默认参数构造异常，certainty 变成 0/NotSent，request_id 变成 0，调用方无法查回真实请求错误。

影响：增量运动等命令可能已经执行，上层却看到“可证明尚未发送”，与 user-manual 的处理建议冲突，可能据此做出不安全的重试决定。原生 unknown_write 闸门仍存在，不能据此宣称库会直接自动重放。也不是所有超时都已发送：初始化期确实可能 NotSent；问题是同步接口把两者合并。

证据：Python fake-native 返回 EXEC_TIMEOUT 时，真实包装器输出 `motor_inc_jog status=-5 certainty=0 request_id=0`；核心发送后超时路径与 FFI 丢失字段路径已逐段核对。C++17/.NET 存在相同默认确定性构造。此项包含包装器动态复现和原生源码证据，未执行真实运动超时实验。

建议：同步包装器使用提交、等待、复制完整 ErrorInfo、最后 release 的路径；保留真实 certainty/request_id。若 C 同步 ABI 继续只返回 status，应增加错误详情出口，不能将缺失信息默认解释为 NotSent。

## F3 / P2：Python 整数窄化绕过目标编号校验，可操作错误电机

位置：[native.py:86](../bindings/python/pmac/native.py#L86)、[core.py:618](../bindings/python/pmac/core.py#L618)、[core.py:626](../bindings/python/pmac/core.py#L626)；DART 同类转换见 native.py:63、core.py:486。

触发：调用 `motor_kill(4294967297)`。Python 任意精度整数未经范围检查进入 ctypes.c_int32，按 2^32 截断为 1；原生只看到合法的电机 1，无法拒绝原始非法编号。

影响：应被拒绝的参数可能变成对另一合法电机的操作。DART 的 command/address 经 c_uint32 也存在同类截断，可把越界地址变成有效地址。

证据：使用真实 CFUNCTYPE ABI 参数转换，输出 `motor_kill(4294967297) delivered motor=1`。未向真实设备发送 Kill。

建议：Python 层在进入 ctypes 前检查整数类型和每个业务参数的实际范围，覆盖正/负溢出边界；不能只依赖原生收到截断值后的范围校验。

## F4 / P2：排空等待超时后遗留 drain 标记，健康成员持续不可派发

位置：[client.rs:987](../crates/pmac-core/src/client.rs#L987)、[pool.rs:425](../crates/pmac-core/src/pool.rs#L425)、[client.rs:653](../crates/pmac-core/src/client.rs#L653)。GitNexus 确认 drain_member 调用者包括 manage 和 resize。

触发：两成员池中，成员 2 正在执行命令；提交 PoolDrain(2)，在等待 Busy 结束期间让管理请求超时、取消或被新 revision 替代。

根因：先置 `m.drain=true`，后续 drive 等待发生错误直接返回，没有回滚标记或接续排空。原命令完成后成员状态恢复 Ready，但仍被调度器拒绝。

影响：成员明细显示两个 Ready，池可用数却只有 1；原亲和线程下一请求得到 NoReadyLane。普通 Repair 只选 Failed/Drained，不能选中这个 Ready 但被禁用的成员。需再完整排空并 Repair/Resize，或重建 Client。

证据：100ms 的排空请求返回 QueueTimeout/NotSent；放行原命令后 `ready=1,busy=0,failed=0`，成员明细两个 Ready；原线程下一只读请求稳定返回 NoReadyLane。

建议：在尚未进入不可逆关闭前，以 guard 回滚排空标记；或显式保留可接续的排空中状态，保证状态快照和 Repair 行为一致。

## F5 / P2：远端先关闭 Channel 时丢失确认，已关闭通道仍占不确定配额

位置：[connection.rs:57](../crates/pmac-transport-russh/src/connection.rs#L57)、[connection.rs:134](../crates/pmac-transport-russh/src/connection.rs#L134)、[session.rs:688](../crates/pmac-transport-russh/src/session.rs#L688)。

触发：远端因 GPASCII 退出等原因先发送 EOF/Close；channel_close 回调先于本地 close_channel 注册等待者执行。

根因：close_events 只通知当时已存在的等待者，没有保存“已经收到 Close”的事实。回收随后注册新的等待者，远端不会再次发送关闭确认。

影响：回收等待 2 秒后把已释放的通道记为 uncertain，占住本地共享配额；反复发生可耗尽八通道预算，需要人工确认恢复。

证据：复用本地 SSH fixture 的 `#3p` 不完整响应并主动关闭场景，得到 `release_uncertain=1, failed=1, gpascii_active=0, closed=1`。期待 release_uncertain=0 的新增测试失败。不是仅用模拟 bool 推导。

建议：按 Channel 保存关闭状态；检查已经关闭和登记等待者必须在同一同步协议下完成，覆盖 Close 先于本地回收的顺序。

## F6 / P2：小响应预算下，分片回显会被误算为响应超长并退役 SSH

位置：[gpascii.rs:55](../crates/pmac-protocol/src/gpascii.rs#L55)、[gpascii.rs:93](../crates/pmac-protocol/src/gpascii.rs#L93)、[session.rs:817](../crates/pmac-transport-russh/src/session.rs#L817)。

触发：max_response_bytes 小于命令回显长度，且回显跨 SSH Data 包，例如命令 Sys.ServoCount、响应仅一个字符、预算为 1。

根因：回显未完整时 strip_echo 会保留其前缀；push 尚未得到 ACK 时立即按响应预算检查整个 buffer，把回显前缀当作响应。

证据：相同字节流 `Sys.ServoCount\r\n1\r\nACK` 整包输入返回 body=[49]；仅先输入前 5 字节 `Sys.S` 即返回 ResponseTooLarge(1)。传输直接使用请求限额构造 decoder，随后将此错误标成 fatal，正常读取失败并退役 SSH 成员。

建议：区分待判定回显与响应正文，回显判定结束前保留独立且有界的命令长度 allowance；测试相同字节流在每个分包点均有相同结果，包括预算小于命令长度的情况。

## F7 / P2：C++98 command 的响应分配异常泄露 Request ID

位置：[pmac_cpp98.hpp:126](../bindings/cpp/pmac_cpp98.hpp#L126)，关键分配在第 135 行。

触发：请求成功完成后，创建 response vector 抛出可捕获的 std::bad_alloc。

根因：该函数用裸 Request ID，vector 分配在显式 release 之前。异常跳过释放，异常对象也不携带可回收的请求 ID。

影响：请求结果与配额保留，Runtime release 可持续返回 Pending。相同文件中的 getResponse 已有 ScopedRequest，可复用其方法；未确认 fileExistsNoThrow 存在相同漏洞。

证据：对 vector 分配注入异常，调用者捕获后输出 `caught bad_alloc; released request count=0`。

建议：成功提交后立刻用 RAII 管理请求 ID，所有分配和复制都在 guard 生命周期内进行。

## F8 / P2：新增 LICENSE 未更新 vendor 清单，依赖校验与 CI 稳定失败

位置：[Test-DependencyPatch.ps1:18](../scripts/Test-DependencyPatch.ps1#L18)、[ci.yml:22](../.github/workflows/ci.yml#L22)、[ci.yml:152](../.github/workflows/ci.yml#L152)、vendor/russh-upstream-files.sha256、vendor/russh/LICENSE。

触发：在当前提交运行 `./scripts/Test-DependencyPatch.ps1`。

根因：校验清单记录 90 个文件，当前 vendor 下有 91 个；唯一额外文件是本次加入的 LICENSE，文件集合校验因此直接抛错。

证据：实际运行退出 1，报 `Vendor source has an unexpected file count`；Compare-Object 确认差异为 LICENSE。CI 的 quality 和安全相关步骤调用同一脚本，因此当前源码会在该门禁失败；本轮未声称已经观察到远端 Actions 失败记录。

建议：保留许可证，并在来源文件清单/补丁元数据或明确的附加文件规则中登记、固定其哈希；不要删除必要许可证来使计数通过。

## 尚未动态验证的风险

以下不计入上述 8 项确认缺陷。

1. **DART 建连故障隔离不足**：session.rs:154 的服务发现成功后，独立 TCP connect 超时/取消时，GPASCII initializing 已为 false；client.rs:538 的隔离豁免仅覆盖 Dart transfer，DartConnect 可落入 fatal 的 fail_member，退役健康 SSH。需要控制服务发现已成功而 TCP connect 阻塞的测试，明确两阶段错误分类。
2. **执行 deadline 不覆盖全部收尾**：client.rs:1018、1047、1079 的关闭/退役 await 在 drive 外。实际传输对单通道关闭有 2 秒上界、SSH 关闭另有 2 秒，但多通道逐个关闭，错误路径的请求终态可能明显晚于 timeout_ms。部分锁获取也没有局部上界。本轮未做黑洞网络下的时延实测，不能声称已有无限挂死证据。应区分执行预算与收尾预算，并明确文档/API保证。
3. **DART Set 先复制后验长度和准入**：ffi/dart.rs:39 在检查 524288 字节上限、方向、地址以及请求预算前就 to_vec。超长但有效的输入缓冲可能触发大量无用分配；分配失败的进程级行为无法由普通 catch_unwind 恢复。建议在借用 slice 阶段完成元数据校验，并使用受控分配/准入。本轮未进行大内存或 OOM 破坏性实验。

## 验证结果与复现记录

基准提交没有变化，第一方生产代码未修改。已执行：

- `cargo test --workspace --locked --offline --quiet`：**161 项全部通过**（11+15+46+37+52）。vendor/russh 有 3 处 RSA 类型别名弃用警告。
- Python 3.9 unittest：19 项中 18 通过、1 个 POSIX fork 用例在 Windows 跳过。
- `dotnet build bindings/dotnet/Pmac/Pmac.csproj --no-restore -v quiet`：通过，0 警告、0 错误。
- vendor 补丁完整性校验：失败，见 F8。
- 额外故障复现：F1、F5 的“期望正确行为”断言均失败；F3、F4、F6、F7 程序输出错误行为；F2 为包装器动态结果加原生错误链源码证明。不能把这些失败复现与原仓库测试全通过混为一谈。

本机复现命令（Cargo/Python 若不在 PATH，使用已安装程序绝对路径或 py launcher）：

```powershell
cargo test --manifest-path target/review-transport/Cargo.toml --offline --target-dir target review_known_hosts -- --nocapture
cargo test --manifest-path target/review-transport/Cargo.toml --offline --target-dir target review_remote_closed -- --nocapture
cargo run --manifest-path target/core-review/Cargo.toml --offline --quiet --target-dir target
$env:PYTHONPATH = Join-Path $PWD 'bindings/python'
py -3.9 target/bindings-review-python.py
./target/review-4cd443c/echo_repro.exe
./target/bindings-review-alloc.exe
./scripts/Test-DependencyPatch.ps1
```

临时复现材料位于 target/review-transport、target/core-review、target/review-4cd443c、target/bindings-review-python.py、target/bindings-review-alloc.cpp。传输 harness 直接引用当前生产源码并复用本地 SSH fixture；这些临时材料不在 Git 中，不作为已提交的永久回归测试。修复时应将适当的故障测试移入正式测试集。

## 优先级与边界

建议先处理 F1 主机密钥撤销和 F2 执行确定性，再修正 F3 错误目标参数及 F8 CI 门禁；其余连接池/配额/分帧/异常泄漏问题随后逐项补回归。F1、F2 是发布前需要重点阻断的问题；测试全通过不足以签收当前版本。

本次是覆盖第一方生产源码的全仓静态审核，加定向动态故障复现；不是对每个外部依赖逐行重审，也不是无缺陷证明。未执行真实控制器运动、PLC/程序加载、物理断网、跨平台 ABI 动态复测、Miri/Loom/Sanitizer 或长期压力试验。未重新拉取安全公告库运行 cargo audit/deny，不能沿用旧报告宣称当前时点依赖无漏洞。

先前提到的 recovery_pending 重复锁疑点已排除：Rust 的惰性布尔操作数具有独立临时作用域，不能将该表达式误报为必然递归加锁。历史 release 准入问题也已由当前 Closed 状态要求修复，不重复计数。
