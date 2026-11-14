# C ABI 契约摘要

[English](abi-contract.en.md)

更新于 2026-11-12。公开定义以 [include/pmac.h](../include/pmac.h) 为准，所有导出使用 `extern "C"`、`pmac_` 前缀；Windows x86 使用 `__cdecl`。

## 版本与结构

- 库版本为 2.0.3，ABI major/minor 为 **2.1**；`pmac_abi_version()` 返回 **2001**。
- 已有输入结构布局保持不变，接受 ABI major 2 的输入，包括版本值 2000；新增 pool/通知接口需相应库导出。
- 公开聚合采用默认 C 布局的 `repr(C)`，禁止 pack(1)。`size_t` 随目标位宽变化，不以 x64 布局代替32位验证。
- 输入结构带 `struct_size`、`abi_version`；先读取8字节公共头并检查最小尺寸与 ABI major，再复制完整输入结构。输出结构必须由调用方按当前公开类型分配完整空间。
- `pmac_get_runtime_info` 返回 ABI、库版本、功能掩码和原生指针位宽。调用方应核对 ABI major 和指针位宽，不能混用不同架构的头文件与动态库。

## ID、关闭与释放

Runtime、Client、Request 的高4位为类型标签，低位是永不回绕复用的序列。释放后旧 ID 永久失效；内部 Arc 保活已取得的对象引用，C 调用不解引用对象地址。

- `pmac_client_close(id, 0, wait_ms)` 停止该 Client 准入，取消已接纳请求并收尾。当前仅支持 close_mode=0。
- `pmac_client_release` 只允许在 Client 已完成 close 且该 owner 的在用、回收中及不确定 GPASCII 资源全部归还后删除 ID。其余情况返回 `SHUTDOWN_PENDING` 并保留 ID；设备核实后调用 `pmac_pool_confirm_recovery`，再重试 release。
- `pmac_client_recovery_ids` 枚举已关闭/关闭中且仍有未归还资源的 Client ID。即使 C++ 析构已移交对象，也可据此取得保留 ID，再按设备核实结果执行受限恢复。
- `pmac_runtime_shutdown` 停止该 Runtime 的新 Client 和新 Request 准入，关闭现有 Client，并通过受监督收尾等待 Tokio 及阻塞任务停止。
- shutdown 超时返回 `SHUTDOWN_PENDING`，Runtime ID 仍有效，允许再次等待。超时不意味着后台任务已经退出。
- shutdown 成功后，本地状态/完成结果仍可读取；调用方仍需释放该 Runtime 的全部 Client/Request ID。
- `pmac_runtime_release` 只有在 shutdown 已完成、全部 Client 关闭且所有子 ID 已释放后成功。否则返回 `SHUTDOWN_PENDING` 并保留 ID。
- `pmac_runtime_cleanup` 启动一次性清理监督者并立即返回；监督者持有 Runtime，等待后台停止和子 ID 后续释放后完成 release。C++ 析构使用该入口，不在调用线程永久等待。
- `pmac_runtime_cleanup_wait` join 已安排的清理监督线程。动态卸载 DLL/so 前，停止新 Runtime/清理准入并调用该接口等待全部监督线程退出；仅删除 Runtime ID 不是卸载屏障。
- SDK Dispose/shutdown 同样保留 pending 的 Runtime ID。先释放仍保有的请求结果/Client，再重试；GC/finalizer 不替代显式完成关闭。

C 的典型收尾顺序：shutdown 并等待成功 → 读取所需终态/结果 → release Request/Client → release Runtime。关闭后不再提交新业务；卸载 DLL/so 前须完成这些步骤，并确保没有调用线程仍在执行 C 入口。

## 请求、等待和结果

`pmac_get_response_async`、`pmac_command_submit` 和其他 submit 共用请求核心。提交成功返回 Request ID；准入失败不返回一个待执行请求。`QUEUE_FULL` 是准入拒绝，不能把一次已接纳操作的超时当成可重提的队列满。

- 总执行 deadline 从提交阶段建立，包含排队；`pmac_request_wait` 的 wait_ms 只限制本次等待。`WAIT_PENDING` 不取消请求、不修改执行状态。
- `PMAC_REQUEST_INFINITE` 只移除总执行 deadline，cancel/close 仍可用。
- 请求拥有复制后的命令/路径/配置；结果按请求保存。完成结果不可被迟到事件覆盖，可由多线程分别复制到各自的缓冲。
- 响应、stderr、错误、正则捕获的复制不会发送新命令。小缓冲返回 `BUFFER_TOO_SMALL` 与所需字节数，使用相同 Request ID 重取。
- 非空输出需有效缓冲；空字节结果允许 null 输出。`required` 必须非空。结果按长度返回，不要求尾随 NUL。
- 文本输入是带长度的 UTF-8，不含 NUL；原始响应允许任意字节。
- 模式精确为空、`number`、`variable` 时保留旧 helper 提取；其他模式执行 `regex::bytes` 并返回真实捕获/语法错误。旧库对其他模式曾直接返回原始响应；新 API 的明确语义见 [模式迁移](pattern-migration.md)。
- 同步 API 只等待同一请求核心，不维护第二套 SSH 或解析逻辑。
- 同步业务调用失败后，须在同一线程、调用其他 PMAC 入口之前调用 `pmac_copy_last_error` 复制错误详情。此接口本身不会清除详情，支持先取所需长度再复制消息。详情包括执行确定性、连接 epoch 和诊断用 request ID；同步调用已释放该 ID，不能再通过它查询请求。
- 新的状态返回型 PMAC 调用会替换线程上的上一条错误；不同线程互不影响。无法取得同步错误详情时，不得推断命令尚未发送。

排队取消/超时保证不发送；发送开始与取消通过同一请求状态仲裁。终态与执行确定性一起发布，不因本地复制缓冲不足而改写远端完成事实。

## 完成广播

`pmac_runtime_wait_completion(runtime, cursor, wait_ms)` 在 Runtime 完成序列改变时返回 `OK` 并更新 cursor；等待预算耗尽返回 `WAIT_PENDING`，不改变任何请求状态。每个等待者使用独立的 uint64_t cursor，不能并发写同一个 cursor。

该序列广播唤醒，不消费结果队列；一个等待者不会取走另一个等待者的通知。`pmac_runtime_wake` 也会推进序列，所以被唤醒后必须检查自己的已跟踪请求，不能把一次通知解释为特定请求成功。先扫描请求再等待序列，poll 与 wait 之间到达的完成仍可观察。

.NET/Python 为每个 Runtime 使用一个有界完成泵；跟踪新请求和退出时主动 wake，正常完成由原生序列通知。完成对象容量和结果内存均受限；结果保留到 release，慢消费者不能无限增长。

## 池管理、路由和故障恢复

池包含固定的有序 control 成员1和应用显式创建的额外成员；每成员为独立 SSH/GPASCII 会话。
每个 Client 的 GPASCII 在线通道预算为 **16**，覆盖该 Client 全部成员的预建、打开中、在用、回收中及释放不确定占用。额度按进程唯一 owner 隔离；同一控制器上的多个 Client 各自计数，库不施加跨 Client 合计 16 的限制。第 17 个申请以 `RESOURCE_LIMIT`、`NOT_SENT` 拒绝。已确认远端释放或所属 owner 的显式恢复确认才归还相应额度；未知释放不能提前归还，也不能由其他 Client 的确认抵消。BufferUpload 的维护通路不消耗在线通道额度。

| 入口 | 契约 |
|---|---|
| `pmac_pool_prepare_async` | 初次提供连接配置和 profile，返回管理 Request；不阻塞健康数据通路 |
| `pmac_pool_resize_async` | 显式调整 target/max/min_ready；保留配置目标，失败/部分可用由状态及请求错误报告 |
| `pmac_pool_repair_async` | 修复指定失效成员；member_id=0选择当前目标范围内需修复的成员；不重建健康成员 |
| `pmac_pool_drain_async` | 停止选定成员新派发并收尾；member_id=0选择额外读成员；不允许 drain 固定 control 成员1 |
| `pmac_pool_repair_maintenance_async` | 显式修复维护面；不由普通请求偷偷重连 |
| `pmac_pool_get_state` | revision、desired、ready/busy/creating/failed/retiring、release_uncertain、恢复闸门和最后创建错误 |
| `pmac_pool_get_members` | 按成员输出 ID、epoch、状态、control角色和最后有效响应时间；capacity/required 的单位为成员个数 |
| `pmac_request_get_route` | 返回请求分配的 member_id 和连接 epoch；成员 ID=0表示尚未分配 |
| `pmac_pool_confirm_recovery` | 只在匹配 expected_revision 的静止状态接受应用确认；确认标志必须非零 |

profile 要求 `1 <= min_ready <= target_size <= max_members <= 255`，连接预算必须非零。总创建/等待预算与普通业务 deadline 分开。管理请求建立新 revision，过期任务不能安装旧成员或覆盖新目标；过期管理操作报告 `SUPERSEDED`。

每个提交线程首次使用 GPASCII 时固定一个健康 member，并在其中独占常驻 Channel；该线程后续普通命令与 `PMAC_REQUEST_READ_POOL` 请求均不迁移。`PMAC_REQUEST_READ_POOL` 只放宽经分类确认的只读命令准入，副作用命令在发送前拒绝。不同线程不共用 GPASCII Channel；跨线程/member 不提供隐式写后读顺序或同时刻采样保证。`open prog` 到 `close` 必须从同一线程提交。

维护屏障等待已接纳工作的边界，待执行/执行中的维护阻止相关新业务准入并返回 `MAINTENANCE_ACTIVE`。控制故障仅退役对应成员；维护故障隔离维护面。默认恢复为应用手动 repair，不因流量或失败自动扩容、重连或重放。

`MAY_HAVE_EXECUTED` 的写操作保留恢复闸门，后续冲突操作报告 `RECOVERY_REQUIRED`。应用核对设备状态后才可设置 `PMAC_RECOVERY_CONFIRM_UNKNOWN_WRITES`。`PMAC_RECOVERY_CONFIRM_REMOTE_RELEASE` 是应用确认远端不确定会话已释放的独立声明，用于归还配额；关闭本地 socket 不构成此证明。正常 Session 关闭先逐 Channel 等待远端 Close；若仍有不确定记录，Closed Client 可在其自身 ID 尚未释放时确认该 owner 的旧通道，不能确认其他 Client 的记录。

成员状态值：Ready=0、Busy=1、Failed=2、Retiring=3、ReleaseUncertain=4、Creating=5、Drained=6。不要仅凭 ready_count 回升自动清除未知写结果。

## 执行确定性与身份

- `NOT_SENT`：可证明尚未发送。
- `MAY_HAVE_EXECUTED`：已经开始发送或失去确认，远端动作可能发生；调用方核对设备状态。
- `CONFIRMED`：取得完整有效协议确认；不表示机械运动或应用程序已经结束。

`PUBLISH_UNKNOWN` 表示文件发布结果不确定。库不自动重试可能有副作用的操作，也不以完整 ACK 代替机械状态判断。

主机身份与登录方式独立。必须选择 known_hosts、32字节 SHA-256 指纹，或显式 `PMAC_CONNECT_ACCEPT_ANY_HOST_KEY`；后者只用于调用方明确接受风险的环境。密码/私钥/口令在异步提交时转为库拥有的配置，不写入错误文本和测试报告。同时配置私钥和密码时不自动兜底密码认证。

- C++ 头文件默认兼容缺少 pmac_copy_last_error 导出的旧版 ABI 2.1 库，失败时使用保守的 MayHaveExecuted。确认所链接的库提供该导出后，可在包含头文件前定义 PMAC_ENABLE_LAST_ERROR_DETAILS 以启用完整错误详情；该选项要求所有翻译单元保持一致。


GetResponse/command 默认开启透传：跳过 online/program 分类及本地程序上下文检查，原文逐行发送。显式关闭透传可启用规则校验：C 使用 PMAC_REQUEST_CLASSIFY（8），C++ 使用 passthrough=false，C# 使用 Passthrough:false，Python 使用 passthrough=False，MATLAB 使用 struct('passthrough', false)。只读池请求自动启用分类；C 显式 PASSTHROUGH（4）不可与 READ_POOL 或 CLASSIFY 组合。基础帧校验和远端 error # 上报始终生效。
