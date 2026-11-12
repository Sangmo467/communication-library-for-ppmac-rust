# Rust + russh 工业通信库重构实施计划

日期：2026-11-11；实施状态更新：2026-11-12。核心、连接池、C ABI 和三语言 SDK 已实施，已完成 Windows x64 自动化及 ARMv7 控制器的短时只读联机验证；逐项证据与未验收范围见 docs/verification.md。用户已将 72 小时/7 天长稳验收排除在本轮范围之外。本文后续指标及阶段退出条件仍为设计要求，不自动代表已验收。

## 1. 目标与范围

重新设计架构，以 Rust、Tokio、russh 实现工业环境使用的 PPMAC 通信库。以功能完整、并发正确、故障语义清楚、低尾延迟和可验证的跨平台行为为优先级，不要求兼容旧 API、ABI、类名或内部实现。

已确认要求：

- 继承现有 C 核心和 C++ 包装层的全部业务能力，唯一明确排除项为 dpkg 安装。
- C ABI 为唯一原生互操作边界，同时交付 C++、C#、Python 调用库。
- 支持 Windows、Linux；支持 x86 32 位、x86-64 64 位，以及 Linux ARM32/ARM64、Windows ARM64。32/64 位分别构建和验收。
- 允许多线程共享连接并提交请求，要求生命周期、取消、断连和结果归属可靠。
- 重点优化命令响应延迟、持续运行稳定性、断线检测和故障后的可恢复性。

工作假设：运行于有操作系统的上位机/工控机或 ARM Linux 主机，不是裸机 MCU。Windows ARM32 不列为首版正式目标；若现场需要，另行评估工具链、依赖和系统可用性。最低 Windows/Linux/glibc 版本、ARM 指令集和具体板卡在 P0 固定，不能用“支持 Linux/ARM”替代明确版本清单。

本库提供有界资源和软实时响应目标；SSH、普通操作系统与控制器服务共同决定时延，不能承担硬实时伺服周期或安全急停链路。保留 Abort/Kill/JogStop 功能，但网络停止请求不构成硬件安全保障。

## 2. 仓库依据与现有行为

本计划依据 README.md、src/pmac_comm_core.h、src/pmac_comm_core.c、src/pmac_send_guard.c、include/pmac_client.hpp、pmac_command_regression_tests.c，以及现有审查记录；另已静态审阅 worker-rearchitecture/ 的 2.0.1-dev 工作树实现、请求模型和回归，迁移结论见附录 E。现有 docs/plans/2026-11-11-per-connection-worker-architecture.md 是面向 libssh2 的独立方案；本计划不将其线程实现作为新库约束。

当前实现的关键事实：

| 现状 | 依据 | 新架构影响 |
| --- | --- | --- |
| 公开 handle 含 libssh2 指针、socket、线程、递归锁及生命周期字段 | pmac_comm_core.h 的 ssh_shell_handle_t | 改为不可解引用的带代次 ID，不暴露 Rust/SSH 内部结构 |
| 同 handle API 由递归锁串行化，不同 handle 可并行 | thread_guard_enter、README 第 9 节 | C 调用安全并发；实际命令执行由协议约束调度 |
| GPASCII 持久通道执行 gpascii -2 | pmac_connect_internal、read_response | 保留持久 GPASCII，验证是否可直接 exec，无须每次新建会话 |
| 普通响应以 ACK 帧结束，含回显处理；部分复位没有普通 ACK | read_response、command_allows_no_ack 等 | 独立增量解码器及复位专用状态机 |
| 当前等待已使用 socket 就绪等待，部分路径上限 2 ms | wait_socket_io、wait_session_deadline | 不能把 2 ms 等待上限直接算作固定时延损失；新旧需实测 |
| 保活默认 30 秒，trylock 失败跳过；want_reply=0 | keepalive_thread_func、libssh2_keepalive_config | 现有成功写入不等于对端回复，重新设计失联检测 |
| 命令发送后超时断开，防止旧响应串入下一请求 | read_response、README 第 7.4 节 | 保留安全原则，并精确隔离失效连接/通道代次 |
| Shell 保留工作目录、环境，使用请求特有完成标记 | execute_shell_command、README 第 7.5 节 | 不能只换成逐次独立 exec 而丢失有状态 Shell |
| 文件先 staging、校验后发布；保持原目标，考虑权限 | Upload、Download、审查修复记录 | 新库必须继承，不能退回直接覆盖 |
| 某些数值接口以 0 表示失败，错误是 handle 上最后一次写入 | 公开状态读取及 GetError 系列 | 改为返回状态 + 类型化结果 + 每请求错误 |

代码索引工具未发现已索引仓库，本机未找到 vexor；本次使用 UTF-8 直接读取及 rg 检查，未安装额外工具、未重建索引。

## 3. 全功能迁移与验收矩阵

下表覆盖能力，不要求保留原符号。P0 还需把公开 C 符号、C++ 公共方法逐一登记至 feature-matrix，关联实现与测试，防止只迁移主流程。

| 旧功能/入口族 | 新库能力 | 验收重点 |
| --- | --- | --- |
| Init、Cleanup/Ex、Create、Destroy/Ex | 显式 Runtime、Client、shutdown、release | 多 Runtime、并发关闭、重复释放、库卸载 |
| Connect、WithProbe、WithKnownHosts | IP/DNS/端口、密码/密钥认证、可选 TCP 预探测、known_hosts/固定指纹 | 连接总预算、主机身份、私钥口令、不同固件算法 |
| GetResponse、FreeRegexResult | 原始字节响应、规范化文本、可选正则及捕获组 | 回显、ACK、分包、空响应、截断、正则能力差异 |
| GetError、GetErrorCopy、GetLibssh2Error | 请求级结构化错误、底层分类/文本 | 多线程错误不覆盖；不再承诺 libssh2 错误数值 |
| Upload、Download | SFTP 流式传输、校验、staging、发布、权限策略 | 原目标保护、空文件、大文件、磁盘满、发布结果不确定 |
| BufferUpload、BufferUploadEx | 上传、校验、gpascii -2 -i 加载、EOF 检查、按原 Ex 能力选择保留/清理文件 | 加载失败、解析失败、长耗时及失败后的文件状态 |
| FileExists、FileRemove | 存在性类型化返回、删除 | 不存在与无权限分开；UTF-8、引号及危险路径 |
| ExecuteShellCommand、C++ Shell 能力 | 持久 Shell；另增隔离 exec | cwd/env 保留、stdout/stderr、退出码、无限等待可取消 |
| Abort | 全坐标禁用命令能力 | 保留 &*adisable 的业务语义，不与请求 cancel 混淆 |
| CoordEnable、CoordExecProg、CoordHold、CoordRun | 坐标系使能、程序运行、保持、继续 | 命令格式、范围、并发顺序 |
| PLCEnable、PLCDisable | PLC 启停 | 参数校验及远端错误 |
| MotorKill、MotorEnable、MotorJogStop、MotorAbsJog、MotorIncJog | 电机操作及浮点参数格式化 | NaN/Inf/溢出拒绝，发送前完整校验 |
| CoordStatus、MotorStatus | 原始 u64 + 类型化状态位 | 全部已暴露位及两位字段与 C++ 当前定义对照 |
| MotorPos、MotorDesPos、MotorVel、MotorPosErr | 实际位置、目标位置、速度、位置误差 | 区域设置无关、非法值拒绝、0 值不是错误 |
| ServoErrorCtr、PhaseErrorCtr、ServoCount、PhaseTime、ServoTime | 系统计数与时序读数 | 位宽、溢出和负值规则 |
| AbortAllFlag、PLCRunning、CoordProgRunning、MotorClosedLoop、CoordMtrEnabled，含 Ex | 明确状态码的布尔查询 | 只接受有效协议值；故障不得当 false |
| IsConnected、C++ probeConnection | 本地状态快照 + 显式远端探测 | 查询不发包；快照带时间与代次 |
| VersionString/Number、AbiVersion、RuntimeInfo | 库版本、ABI major/minor、能力及构建目标查询 | 头库错配、绑定错配可识别 |
| C++ RAII、异常、Strict/NoThrow、状态位解码 | 新 C++ SDK 的 RAII、状态返回/异常选项、完整解码 | 业务能力完整，不复制旧的“失败为 0”歧义 |
| save、$$$、$$$***、EtherCAT reset/rescan/slaves 等长命令行为 | 普通透传 + 明确维护操作策略 | 超时、无 ACK、复位后 readiness 重建 |
| InstallDpkg、installDpkg/NoThrow | 按用户要求移除 | 不交付 dpkg 工作流、专用状态码和安装验证 |

旧命名别名不是额外业务功能。正则能力必须保留，但 Rust regex 不支持任意回溯/环视等语法；P0 收集旧调用方表达式并测试现有匹配器的实际语义，再决定主引擎和受限扩展引擎，不以引擎替换悄悄删减能力。

## 4. 推荐总体架构

调用路径：C++ / C# / Python → C ABI → 句柄注册表与请求层 → 设备协调器 → 各连接/通道任务 → russh → TCP。

采用共享的、显式管理的 Tokio Runtime；每设备一个逻辑协调器，连接和通道由异步任务管理。协调器只负责接收请求、状态和调度，不能在处理一次长请求时停止消费控制消息。russh 内部协议循环负责 SSH 传输；应用任务拥有通道的业务顺序，不让全部任务竞争一把覆盖网络等待的大锁。

### 4.1 连接拓扑

- 控制面：按设备配置小型、预建、有限额的 GPASCII 连接池，最低保留一个常驻 lane。每个池成员包含 SSH/通道、独立 gpascii 会话、解码器及代次；每 lane 任意时刻最多一条业务请求等待响应。具体池策略见附录 B。
- 维护面：单独的 SSH 连接承载 SFTP、持久 Shell、隔离 exec、buffer 加载，按需连接并可复用。
- 可选监测/停止面：经过固件多会话语义及压力测试后，在连接池总额度内分配专用 lane；不得在普通请求耗尽池时无上限临时扩容。
- 资源受限设备可选择单 SSH 多通道模式，但必须标明：通道共享 TCP、有序传输、rekey 和连接故障，无法保证消除文件流量造成的队头阻塞。

默认推荐控制/维护双连接，不意味着控制器 CPU、网口与设备共享资源也被隔离。P0 必须测量 SSH 最大会话数、控制器负载、多 gpascii 进程及命令上下文行为；能力不足时显式选择降级模式并降低延迟承诺。

### 4.2 线程与运行时

- Runtime 默认从 2 个 worker 起评估，允许显式 1/2/4/N 配置；ARM 双核/低功耗板以实测选择。拒绝按调用线程或按 API 创建运行时。
- C 入口做参数校验、短注册表查找、输入复制和 try-submit；同步 API 在调用者线程等待请求完成，不阻塞 Tokio worker。
- 异步 API 返回 request ID；同步只是相同请求核心的适配，不维护第二套协议逻辑。
- 本地文件 I/O、密钥文件读取及较重计算使用容量受限工作池；连接前预读配置，不能在 russh Handler 中做磁盘操作或调用用户代码。
- russh Handler 仅更新/转发事件，短时间返回。通道接收任务持续读取；即使调用方暂不取结果，也不可无限阻塞 SSH 协议任务。
- 网络数据、控制消息、完成结果分别限流；每轮限制处理字节/事件数量并主动让出，防止 SFTP、日志或高频提交饿死定时器。
- 先用有界 mpsc、oneshot/完成对象、Arc 和短锁。只有性能证据证明必要时才引入无锁队列、对象池、线程绑核或每设备独立 Runtime。

### 4.3 模块建议

工作区放在 rust/，与当前 C 版本并存：pmac-protocol（纯解析/格式化）、pmac-core（请求/调度/状态）、pmac-transport-russh（连接/认证/通道/SFTP）、pmac-ffi（唯一 unsafe 边界及 C 导出）、pmac-testkit（模拟服务/故障注入）。bindings/ 下分别为 cpp、dotnet、python。

crate 边界按依赖隔离，避免过度拆分。协议层不依赖操作系统和 Tokio；可单测、模糊测试、回放旧抓包。仅传输层依赖 russh，未来升级依赖不穿透到公开 ABI。
## 5. 并发、生命周期和请求语义

### 5.1 安全句柄

C ABI 使用非零 uint64_t ID，Runtime、Client、Request、Result 分别有类型标识；内部为带 slot generation 的注册表 + Arc。C 调用不解引用对象地址，因此关闭后误用旧 ID 能返回 INVALID_HANDLE，不触发裸指针 UAF。代次耗尽时退休 slot，禁止回绕后重新接受旧 ID。

注册表使用短锁取得 Arc 后立即释放；不持锁等待网络、不以未经评估的全局锁串行全部设备。64 位 ID 在 32 位 CPU 上不依赖 AtomicU64 必然可用，按目标能力使用锁或经验证的原子实现。

同一 Client 的提交与 close 有明确线性化点：close 标记 Closing 之后的新请求一律拒绝；此前已接纳请求按关闭策略完成/取消。release 使 ID 失效但内部对象由在途引用保活，不要求用户用裸指针配合猜测销毁时机。

shutdown 负责停止接纳、取消/收尾、关闭连接和等待任务。超时返回 SHUTDOWN_PENDING，并保留可再次等待的 Runtime ID；不得宣称所有后台工作已退出。DLL/so 卸载前必须达到 shutdown 完成，否则后台线程仍会执行库代码。不能用 Tokio shutdown_timeout 返回等价于“所有线程停止”，也不能依赖终结器无限等待。

### 5.2 请求与背压

每请求记录 request_id、client_id、connection_epoch、channel_epoch、接纳序号、操作类型、调度类、单调绝对 deadline、输入所有权、取消令牌、结果和各阶段时间。

初始可配置预算：每设备待执行请求 64 个、输入总量 1 MiB、普通单响应 1 MiB、同时文件任务 1 个；这些仅为待测起点。还需限制全局连接数、请求数、结果保留字节、事件队列、SFTP 在途窗口、完成对象数量。32 位地址空间单独验收。

预留完成元数据预算后才接纳请求；队列满立即返回 QUEUE_FULL。取消/关闭不得依赖普通队列还有空位，使用独立控制通知与共享取消状态。文件按块流式处理。完成且未释放的请求依然占预算，慢消费者不能造成无界增长。

异步入队前复制命令/路径/配置；不能保留调用方栈缓冲或托管指针。请求结束前由库拥有结果，调用方使用 copy API 或显式 Result 租借；释放结果后不得继续访问借用数据。

### 5.3 状态与执行确定性

请求状态：Queued → Sending → AwaitingResponse → Completing → Completed。超时、取消、远端完成和断连竞争时只允许一个终态发布；结果不可被迟到事件改写。

返回值必须分开表达：

- 调用状态：OK、INVALID_ARGUMENT、QUEUE_FULL、QUEUE_TIMEOUT、EXEC_TIMEOUT、CANCELLED、DISCONNECTED、PROTOCOL_ERROR、REMOTE_ERROR、BUFFER_TOO_SMALL、INVALID_HANDLE 等。
- 执行确定性：NOT_SENT、MAY_HAVE_EXECUTED、CONFIRMED。写入 russh 本地队列不是远端执行确认；无法证明一个字节都未发出时保守标 MAY_HAVE_EXECUTED。
- 协议确认与设备动作完成分开：收到 GPASCII ACK 只证明命令交互完成，不证明电机到位、程序结束或安全状态满足。
- 原始响应、远端退出码、解析值、诊断文本、epoch 和阶段耗时随请求保存。

排队超时/取消必须保证不再发送。发送中或发送后取消只是终止等待与传输处理，不能撤销远端动作。默认将失去完整响应边界的控制连接退役，标记相关在途操作结果未知，取消旧 epoch 队列；重建并检查 readiness 后才能接受新业务。

后续若证明仅关闭 GPASCII 通道即可完全隔离迟到响应，可优化为通道重建；必须验证 russh channel ID 生命周期、旧任务退出及数据路由。初版优先完整退役受影响 SSH 连接；维护连接故障不自动关闭独立控制连接。

### 5.4 顺序、优先级和维护屏障

同一 GPASCII lane 接纳后按 FIFO 执行；多线程没有天然业务先后，需要 request dependency 或 transaction/batch 明确关联。队列接纳顺序可查询。单批次不可与其他命令穿插，但不等于设备事务，也不自动提供回滚。

独立监测连接返回的数据不保证与控制命令因果一致；要求“写后读”的请求必须在同一 lane 或显式等待先行请求完成。重连期间不缓存运动命令，不自动重放已发送或结果未知的命令；默认读请求也返回失败，由调用方明确重新提交。

stop 类命令可有独立有界高优先级队列，但只能在帧边界切换，不能插入当前命令响应。是否启用会改变普通 FIFO，须在配置中明确。专用 stop 连接是可选优化，必须验证多会话设备行为；即使如此也不能承诺穿透控制器内部阻塞。

save、复位、buffer 加载等使用设备级维护屏障：停止相关命令接纳/派发，等待安全边界，执行维护，重新验证状态后恢复。其影响可能跨控制和维护连接，不能仅按 TCP 连接隔离。队列保留/取消策略明确，禁止把 reset 前旧命令延后执行到新设备状态。

## 6. 协议与业务实现

### 6.1 GPASCII

实现字节级增量状态机：输入规范化、合法长度校验、可选回显识别、正文、ACK/错误、特殊完成状态。覆盖一个标记分多包、多帧同包、CR/LF 变体、无正文、错误行、stderr、EOF、残留输出和超大响应。保存未消费尾部，不将一次 SSH Data 当成完整响应。

优先尝试不分配 PTY 的 exec(gpascii -2)，减少终端回显与提示符依赖；以实际固件验证为准。若必须交互 Shell/PTY，封装初始化兼容策略，精确控制 echo、换行和 ready 判定，不在热路径反复匹配 Shell 提示符。

复位单独状态机：开始维护 → 发送 → 预期 EOF/重开输入/重启 → 等待恢复 → 重连 → GPASCII ready。保留旧库已支持的无 ACK 复位能力，但不能将旧 250 ms 静默窗口泛化为所有命令的成功判据。save/EtherCAT 等由明确策略设置预算，不任意搜索子串后悄悄延长用户总 deadline。

严格数值解析：u64 状态字、计数溢出、有限浮点、0/1 布尔、完整消费输入、区域设置无关。普通数值读写不经正则、不创建多余中间字符串；原始字节路径与可选规范化文本分离。

### 6.2 Shell

两种能力都提供：持久 Shell 保留 cwd/env，逐次 exec 提供隔离环境、stdout/stderr 和退出码。持久 Shell 每通道仅一条请求在途，使用随机 nonce + 单调序号的开始/结束帧，包含退出码，增量解析，防普通提示符文本误结束。

对 Shell 命令修改 stdout、exec 替换进程、exit、后台任务和无限输出明确边界：失去标记后报告结果未知并退役该通道，不能继续猜测输出归属。后台任务输出可能污染后续请求，默认不对这类用法承诺普通交互语义。

默认有总 deadline；显式允许无限执行，但 close/cancel 始终可用。输出可有界收集或流式消费；收集上限触发明确错误并有界 drain，无法恢复边界时关闭通道，不能为了 drain 永久不返回。流式消费者落后时选择暂停/截断/取消，并暴露丢失信息。

### 6.3 文件与 buffer

SFTP 使用 russh-sftp，验证远端版本与扩展；不能假定 rename 覆盖、posix-rename、fsync 都存在。上传流程：参数预校验 → 同目录独占 staging → 分块上传并校验 → close 及必要 sync → 校验远端 → 发布 → 清理。

保留 MD5 兼容能力；可协商 SHA-256，不能要求旧控制器突然具备新工具。MD5 用于继承原有传输一致性校验，不作为安全认证。优先使用 SFTP 扩展；不支持时使用已验证的远端工具流程。所有命令参数可靠引用，路径不得拼接成可注入语句。

下载同样写本地 staging、close/校验后替换，Windows 与 Linux 分别实现目标存在、文件被打开、权限和同文件系统重命名规则。断电持久性与原子可见性是两回事，严格持久模式须验证文件与目录同步能力，不把 rename 成功宣传为断电后绝不丢失。

发布前失败保留原目标；发布时断连返回 PUBLISH_UNKNOWN，不自动重试覆盖。按旧能力验证权限/所有者保持；本地上传源被修改、远端下载源被修改应检测或要求稳定快照，并在结果中说明验证范围。

文件吞吐使用有界 pipeline；评估块大小 16/32/64/128 KiB、在途 1/4/8 档，默认由控制延迟和内存预算选择。buffer 加载保留 EOF 标记检查、文件保留/删除选项，加载操作纳入维护屏障。删除只是清理步骤，不能覆盖原始业务错误。
## 7. 断线检测、健康与恢复

分开暴露传输状态、GPASCII ready、维护通道状态、设备业务健康。SSH 活着但 GPASCII 卡死时，不得显示“业务正常”；一次 read/write 成功不代表下一次操作一定可达。

### 7.1 多层检测

| 层次 | 方法 | 判断能力与限制 |
| --- | --- | --- |
| TCP/SSH 被动故障 | 持续消费 EOF、close、disconnect、读写错误及协议任务退出 | 对显式错误快速响应；黑洞可能没有事件 |
| SSH 保活 | russh keepalive_interval + keepalive_max，检查回复语义 | 证明 SSH 对端有响应，不证明 GPASCII 服务健康 |
| 请求 deadline | 从 API 接纳前建立单调总预算 | 可识别命令不响应；不能区分网络丢包和设备繁忙 |
| 应用探测 | 空闲时发送已验证无副作用的 GPASCII 查询 | 与业务共用 lane，不能打断在途帧；忙时不伪造探测成功 |
| TCP keepalive | socket 参数作为后备 | 不作为主要秒级检测时限依据 |

russh 文档定义 keepalive_interval 为“从服务器未收到数据的间隔”，而非严格周期；其他通道持续发数据可能影响触发。因此不直接写 T=interval×max 的硬保证。P0 阅读固定版本源码并通过双向/单向 DROP 实测，确认触发、计数、reply failure、任何数据是否复位计数及 rekey 行为。

初始档位供实测：一般环境 SSH interval=1 s、max=3，空闲业务探测 interval=1 s、deadline=500 ms；低抖动局域网可评估 250 ms/3 次，但不能不测控制器负载就默认启用。普通请求预算按业务设置，不能一律 500 ms，reset/save/加载另有维护预算。

检测指标以“故障注入时间 → 本地状态转移/请求结果发布/事件可取”分别计量。目标在基准局域网环境黑洞检测 p99 ≤4 s；显式 RST/EOF 的本地处理额外延迟 p99 ≤20 ms。均为待验证目标，跨平台分别给证据；若要求百毫秒级黑洞检测，需要在 P0 重新选择探测预算及承载方式。

### 7.2 状态机和重连

连接状态：Created → Connecting → Authenticating → Initializing → Ready → Suspect/Recovering → Disconnected；关闭进入 Closing → Closed。Ready 需要身份认证、GPASCII 初始化及有效查询通过；维护面另有状态，不强迫上传连接失败就把控制面判死。

所有事件带 connection/channel epoch、本地单调时间、原因和最后有效业务响应时间。重连产生新 epoch，旧结果只允许完成其原请求；旧缓存、旧输出、旧取消和旧定时器不能作用于新连接。状态快照与事件必须可对照；事件队列溢出时记录 gap 计数，关键状态仍能通过快照恢复。

重连默认由策略显式开启，可自动恢复传输，禁止自动恢复运动执行。退避例如 100 ms → 200 ms → 500 ms → 1 s → 2 s → 5 s，带 jitter 和最大尝试/总恢复预算；认证错误和主机密钥变化不进入高频重试。DNS、TCP、SSH、认证、ready 各阶段预算均受连接总 deadline 约束，关闭能中止等待。

断连后所有旧 epoch 请求确定完成，报告 NOT_SENT 或 MAY_HAVE_EXECUTED；不把旧排队运动带到重连后。自动恢复成功仅报告 Ready，应用重新核对坐标系、电机、PLC 和程序状态后决定继续。缓存值带 epoch、采样时间、有效期和 stale 标记，默认同步查询不偷偷返回缓存。

## 8. C ABI 与语言 SDK

### 8.1 ABI 规则与接口草案

所有函数 extern C，Windows x86 明确 __cdecl。导出 pmac2_ 前缀，与旧库并装；新 ABI 从新 major 起步。公开结构 repr(C)，带 struct_size、abi_version、保留字段；以指针传入/返回聚合，避免跨编译器按值聚合 ABI 差异。不得在 ABI 中暴露 Rust enum/bool/String/Vec、C++ STL、异常、平台线程类型或原生 SSH 指针。

固定宽度整数与 int32_t 状态；长度在 C 侧使用 size_t 并按位宽绑定，序列号/ID 用 uint64_t，布尔值约定 uint32_t 0/1。不能假定所有 32 位平台 u64 对齐相同，C/Rust/C# 使用各自目标默认 C 布局并逐字段 offset/size 对照；不使用 pack(1) 掩盖差异。网络字段按协议编解码，不依赖主机端序和未对齐指针访问。

以下仅为接口族草案，P1 冻结签名：

```c
typedef uint64_t pmac2_runtime_id;
typedef uint64_t pmac2_client_id;
typedef uint64_t pmac2_request_id;

int32_t pmac2_runtime_create(const pmac2_runtime_config*, pmac2_runtime_id*);
int32_t pmac2_client_create(pmac2_runtime_id, const pmac2_client_config*, pmac2_client_id*);
int32_t pmac2_connect_submit(pmac2_client_id, const pmac2_connect_options*, pmac2_request_id*);
int32_t pmac2_command_submit(pmac2_client_id, const uint8_t*, size_t,
                            const pmac2_request_options*, pmac2_request_id*);
int32_t pmac2_request_wait(pmac2_request_id, uint32_t wait_ms, pmac2_request_info*);
int32_t pmac2_request_cancel(pmac2_request_id);
int32_t pmac2_request_copy_response(pmac2_request_id, uint8_t*, size_t, size_t* required);
int32_t pmac2_request_copy_error(pmac2_request_id, pmac2_error_info*, char*, size_t, size_t*);
int32_t pmac2_request_release(pmac2_request_id);
int32_t pmac2_client_get_state(pmac2_client_id, pmac2_state_snapshot*);
int32_t pmac2_client_close(pmac2_client_id, uint32_t close_mode, uint32_t wait_ms);
int32_t pmac2_client_release(pmac2_client_id);
int32_t pmac2_runtime_shutdown(pmac2_runtime_id, uint32_t wait_ms);
int32_t pmac2_runtime_release(pmac2_runtime_id);
```

wait_ms 是本次等候预算，超时返回 WAIT_PENDING，不等于执行 deadline 到期，也不自动取消命令。request_options 单独定义总执行预算和无限模式；同一请求允许多线程 wait 和只读 copy，完成结果不可变。release 后的查找失败，已经取得引用的 copy 安全结束；同一个输出缓冲并发使用仍由调用方负责。

小响应提供 command_sync_into 及类型化同步接口，避免先查长度再重发命令。BUFFER_TOO_SMALL 返回 required 并保留同一请求结果供重取，绝不能为补大缓冲而重新执行命令。正常路径可复制到调用方缓冲；异步工作线程始终不持有该缓冲。

Host/path/config 文本明确 UTF-8 + 长度，含 NUL 的路径拒绝。Windows 本地路径转换为 Unicode；Linux 非 UTF-8 文件名若需支持，另提供 bytes 路径接口。原始响应允许任意字节，文本解码失败不能静默替换成乱码；原始数据保留，可选严格解码或显式替换策略。

每个导出入口边界捕获可恢复的 Rust panic，返回 INTERNAL，并将受影响连接/运行时标记不可继续使用。异步 task 的 panic 通过 JoinHandle 监督转换为同类故障。catch_unwind 不能修复无效 C 指针、访问冲突、OOM abort 或进程终止；FFI unsafe 仅限最小拷贝/校验层并专项审查，不宣称 Rust 能保护任意坏指针。

### 8.2 C++ SDK

提供头文件/可选小型链接库，RAII Runtime/Client/Request/Result，move-only 所有权，共享 Client 由核心支持。同步、future/异步、cancel、shutdown，错误对象携带确定性及 request ID。默认提供明确状态返回，可选异常适配。

核心能力以 C++17 为新 SDK 起点；当前仓库 C++98 下游另交付轻量同步适配头（RAII、错误、完整业务调用），不强制这批用户迁移编译器。异步现代 API 不强行塞入 C++98。C++ 不越过 C ABI 共享 STL/内存分配器，不单独缓存一个可能过时的 connected 真值。

### 8.3 C# SDK

P/Invoke 包装 C ABI，调用约定和 uint64/UIntPtr 明确；封装 IDisposable/IAsyncDisposable、CancellationToken 和 Task。代次 ID 不是裸地址，使用专门的 owned-ID 包装并通过原子状态防止重复 release，不能直接把 uint64 塞入 32 位 SafeHandle/IntPtr。

异步首版可使用容量受限完成泵 + 批量获取完成通知，禁止每个请求占用一个 ThreadPool 阻塞 wait。Task 取消转成 request_cancel，并保留 MAY_HAVE_EXECUTED。绑定在 await 期间持有请求所有权，用户提前 Dispose 不会导致回调使用失效托管对象。

原生库不回调用户业务代码，事件由 SDK 消费后派发；若后续引入 reverse P/Invoke，单独规定委托保活、异常拦截和 UI 同步上下文。终结器只做非阻塞 release，显式 Dispose/Shutdown 完成等待。

### 8.4 Python SDK

首版使用 cffi 的 ABI 模式或经验证会释放 GIL 的 ctypes CDLL 包装共享 C ABI；不再实现第二套 PyO3 SSH 栈。提供 Client/Request 上下文管理器、同步方法、asyncio Future、结构化异常和全部业务操作。

阻塞同步等待期间释放 GIL并实测多 Python 线程调用。asyncio 使用有界完成泵 + loop.call_soon_threadsafe 发布结果，不让每个 await 启一个常驻阻塞线程。Python 取消/对象销毁与原生请求引用分开处理，进程退出前停止完成泵并 shutdown。

返回 bytes 为无损默认，可选严格 decode；禁止自动以本地系统编码解释控制器字节。核心初始化后 fork 的子进程不能复用继承的 Runtime/Client，检测 PID 变化后拒绝；多进程推荐 spawn 并各自初始化。

### 8.5 绑定共同交付标准

三种 SDK 都从相同 C ABI 覆盖全部业务能力和功能矩阵，不只提供原始 command。共享错误分类、超时与取消语义、状态位常量测试、请求追踪 ID；批量与流式接口同样可达。提供同一场景在 C/C++/C#/Python 的示例，验证返回内容逐字节一致。

## 9. 跨平台、32/64 位与性能

### 9.1 发布目标

| 系统 | CPU/位宽 | 建议 Rust target | 发布形式 |
| --- | --- | --- | --- |
| Windows | x86 32 | i686-pc-windows-msvc | DLL + MSVC 导入库；MinGW 下游另验生成导入库 |
| Windows | x86-64 64 | x86_64-pc-windows-msvc | DLL + 导入库 |
| Windows | ARM64 64 | aarch64-pc-windows-msvc | 原生 ARM64 DLL + 导入库 |
| Linux | x86 32 | i686-unknown-linux-gnu | so + 可选静态库 |
| Linux | x86-64 64 | x86_64-unknown-linux-gnu | so + 可选静态库 |
| Linux | ARMv7 32 hard-float | armv7-unknown-linux-gnueabihf | so + 可选静态库 |
| Linux | ARM64 64 | aarch64-unknown-linux-gnu | so + 可选静态库 |

这些是首版计划目标，不是现有已验证产物；交叉编译成功不代表可发布。ARM soft-float、ARMv6、大端、musl、Windows ARM32/旧 Windows 单独建立需求，不从“ARM”自动推导支持。P0 必须冻结最低 OS、glibc、Python/.NET 版本；现有 Linux32 老环境尤其要验证 Rust 标准库和加密后端，不能默认能在旧 GCC/glibc 上直接运行。

同一进程只能载入匹配架构/位宽的库。64 位系统运行 32 位应用时使用 32 位库并验证相应系统兼容层；不能把“64 位系统”当成“应用必为 64 位”。ARM64 Windows 的 x64 仿真与原生 ARM64 分开测量，仿真结果不作为原生性能证据。

Rust target 支持级别、最低系统及依赖支持需按固定工具链的官方平台表复核；见 [Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html)。

### 9.2 依赖与构建

P0 以已核实文档的 russh 0.65.1、russh-sftp 3.0.1 为候选，不直接按 README 中可能滞后的示例版本锁定。锁定 Cargo.lock、Rust toolchain、具体 feature 集、加密后端、构建镜像/sysroot 和 SBOM。

russh 使用 Tokio，且需要选择加密后端。ring/aws-lc-rs 的所有目标编译、运行、CPU 指令需求和尾延迟必须实际对比；不承诺全依赖纯 Rust 或完全没有 C/汇编工具链。按目标可选不同后端，但保持协议与对外行为相同，发布清单明确差别。[russh 官方仓库](https://github.com/Eugeny/russh)

默认禁用压缩，保留经过现场验证的算法交集；旧固件弱算法仅用显式兼容配置开启，记录协商结果。主机密钥默认严格校验，按可信流程录入；如果必须复现旧库不校验能力，提供显式风险模式，不以接受全部密钥作为默认。

Windows 交付尽量单 DLL，审计 CRT/系统 DLL 依赖；Linux 使用最低支持 sysroot 构建，检查 GLIBC 符号和 libstdc++/libgcc 等实际依赖。静态库不是把 rlib 改名，必须验证最终 C 链接的系统依赖与许可证。

### 9.3 延迟优化顺序

1. 持久连接、持久 GPASCII、TCP_NODELAY 显式开启；若自己创建 TCP 流再 connect_stream，直接设置 socket，不能假定 Config 对任意自定义流都生效。
2. 控制与维护分连接，减少串行队列中的长请求，观测 p99/p99.9 后再决定额外 lane。
3. 无 sleep 轮询，等待由 I/O/完成通知/单调计时器唤醒；Windows 不依赖全局 timeBeginPeriod 获得宣传指标。
4. 短命令输入一次所有权复制，增量解码复用缓冲，数值热路径跳过正则和重复解析；输出零拷贝只在明确租借寿命下提供。
5. 后台采样支持用户显式批量查询或订阅缓存。返回采样时间/epoch，批量不保证同一控制周期快照；不能静默合并有副作用命令。
6. Release 优化、LTO、codegen 参数在各 CPU 测试；通用发行版不用 target-cpu=native，避免工控机非法指令。硬件专用优化另出清晰标记产物。
7. 先测量再考虑绑核、调度优先级和分离 Runtime。ARM 调频/温度降频、弱内存顺序、低带宽、无 AES 加速，以及 Windows 调度与 Linux epoll 差异都纳入结果。

russh 的 Config 提供 nodelay、keepalive、窗口等设置，但不是端到端延迟保证。[配置文档](https://docs.rs/russh/0.65.1/russh/client/struct.Config.html)
## 10. 数据正确性与操作边界：强制不变量

以下为用户明确要求，不是可选性能优化。可靠性失败时停止并报错，禁止通过猜测、隐式重试或复用旧数据继续运行。

1. 请求与响应一一归属。每条 GPASCII lane 最多一个在途命令；内部 ID/epoch 不代表控制器协议具有远端 request ID，不能因此启用无依据流水线。
2. 电机和坐标系指令必须自带完整目标。所有相关操作、查询及复合命令的每个相关子句都显式携带电机/坐标系信息，不能依赖上条命令遗留的 #n 或 &n 选择状态。独占执行也不能豁免这条规则。
3. 类型化入口由库生成完整命令。原始 GPASCII 入口使用明确的命令分类与语法检查，拒绝缺少目标、目标不合法和无法确认作用范围的电机/坐标系操作；不能靠字符串“含有 # 或 &”作为检查。P0/P1 收集实际固件命令语法并制定规则，对复合、分号、多行、缩写和嵌套解析重点审查。确属设备全局的 Abort 等操作使用显式全局类型，不伪装成单电机操作。
4. 任意命令透传能力保留，但并不代表接受任意模糊作用域的运动文本；不满足明确目标契约的旧调用必须改写，纳入迁移清单。之前讨论过的独占/批次仅用于防止穿插，不允许借此继续依赖隐式电机或坐标系上下文。
5. 完整帧确认后才发布有效结果。EOF、超时、部分响应、缓冲不足、校验不一致、非有限数字、溢出或解析歧义必须返回显式状态，不补默认值、不把截断结果标记成功。
6. 同一响应最多完成一次；旧连接的数据不能成为新连接响应。异步通知、超时、取消、状态缓存均带代次；清理也要检查代次。
7. 不重复操作。缓冲重取只复制同一结果；重连、取消、超时、ACK 丢失均不能自动再发可能有副作用命令。SSH 不能保证断网场景远端 exactly-once；如果未来需要此保证，必须增加控制器侧持久化请求 ID/去重协议。
8. 错误不伪装成数值。布尔/计数/位置结果必须同时有有效标志与状态；缓存必须标 stale，状态字最高位和多位字段不丢失。
9. 内存与数据寿命明确。异步核心不写调用方过期内存；ID 失效后不会变成另一个设备；用户错误不覆盖其他请求的诊断。
10. 所有能力有书面契约。输入范围、缺省值、超时单位、并发顺序、重连、关闭、输出容量、路径编码、远端副作用及错误后的状态逐项定义；不明确的情况拒绝执行并返回可诊断错误。

“不允许未定义行为”应落实为可验证设计：核心 safe Rust，最小 unsafe FFI，严格布局/所有权检查，禁止越界/数据竞争/未初始化读取，必要处整数 checked 运算，ARM 不依赖 x86 偶然成立的内存顺序。C 调用方仍须提供真实有效的指针及缓冲长度；任意伪造地址无法由同进程 C ABI 安全探测。官方 SDK 保证正确封装；若需要隔离不可信 C 客户端的内存错误，应另采用进程隔离/IPC，不能声称 catch_unwind 能消除此类 UB。

这也不等于断网后可以知道所有远端事实：MAY_HAVE_EXECUTED/PUBLISH_UNKNOWN 是明确定义的结果状态，触发核对流程；禁止把未知事实猜成成功或失败后继续执行。

## 11. 性能指标与测量方法

### 11.1 分段测量

每请求采集 t_enter、t_admitted、t_dispatched、t_write_accepted、t_first_byte、t_frame_complete、t_result_ready、t_caller_return。write_accepted 仅表示本地传输层接纳，不能命名为远端接收时间。

分别统计：C ABI/SDK 开销、队列等待、调度、协议响应时间、本地解析/完成开销、总延迟。以原 1.0.40、固定源码版本的 worker-rearchitecture 和 Rust/russh 三者作对照，使用同一控制器、固件、网络、算法、命令及负载，区分冷启动和连接预热；C 与三个 SDK 分开出报表。

至少报告 p50/p95/p99/p99.9/max、超时比例、吞吐、CPU、RSS、分配次数、队列长度、文件吞吐、检测时延和控制器负载。使用固定速率开环压测，避免只用闭环压测掩盖排队长尾；成功和失败请求均计数，不能丢掉超时后再声称低 p99。

### 11.2 初始目标与签收

| 项目 | 初始目标，均需 P0 基线后冻结 |
| --- | --- |
| 数据串包、结果错归、错电机/坐标系、重复副作用 | 验收测试中必须 0；任一出现即停止发布 |
| 命令尾延迟 | 同平台同环境 p99 不劣于旧库，优先争取降低 20%；不能只比较均值 |
| 本地完成路径 | x64/ARM64 基准机 p99 ≤200 μs；ARM32 初始 ≤500 μs，另记录调度噪声 |
| C ABI submit 基础开销 | 无网络/无锁竞争 microbenchmark p99 初始 ≤20 μs；不混作业务响应承诺 |
| 并发 | 每设备 1/2/8/32 调用线程、多设备 1/8/32，队列过载明确拒绝且不乱序 |
| 文件传输干扰 | 双连接模式传输期间短命令 p99 增幅初始 ≤20%，控制器饱和情况单独报告 |
| 执行 deadline 处理 | deadline 已到后本地终态发布额外 p99 初始 ≤20 ms，平台逐项测量 |
| 黑洞/RST | 按第 7 节目标验收，测正反向单向及双向故障 |
| 长稳 | 每主架构连续 72 小时混合负载；代表平台 7 天，句柄/任务/内存无持续泄漏趋势 |
| 关闭 | 无阻塞系统调用场景初始 ≤1 s；无法完成时明确 SHUTDOWN_PENDING，不假成功 |

上述微秒/毫秒目标是工程预算，不是已实现事实，也不保证所有旧设备相同绝对数字。P0 给每个发布档位填写硬件、OS、系统电源模式、RTT、服务时间、可达目标和证据；不达标时先定位设备/调度/库瓶颈，再调整实现或透明调整签收范围。

吞吐上限受单 lane 服务时间限制：λ 持续接近或超过服务速率时，队列时延必然增加。优先减少业务轮询、显式批量和过载拒绝，不能用扩大无界队列“解决性能”。

## 12. 验证与发布门禁

### 12.1 测试分层

- 协议回放：以旧实现真实响应样本、回归用例和固件文档建立 golden corpus。全分割点分包、合包、CR/LF、回显、ACK、无 ACK、错误、超长、乱码、半帧 EOF；新旧结果差异逐项签收。
- 属性测试/模糊测试：任意输入不越界不崩溃不无界增长；帧分割方式不改变输出；格式化/解析可对照；所有目标编号和状态位含边界覆盖。
- 并发状态模型：Loom 等测试小型自有同步组件，覆盖 close/submit、release/wait、cancel/complete、deadline/response、断线/reconnect；不声称模型工具覆盖 russh 或整个系统。
- FFI：C 编译布局断言、Rust size/align/offset、C# Marshal 对照；重复释放、失效 ID、长度溢出、合法空指针组合、小缓冲、32/64 位 cdecl 验证。Miri 用于支持的纯逻辑/unsafe 边界测试，Sanitizer 用于支持的平台，不拿工具不支持当成功。
- 可控 SSH/SFTP 服务：认证失败、host-key 变化、窗口耗尽、非法包长、无回复、rekey、任务 panic、通道异常关闭，验证请求完成一次和资源回收。
- 网络注入：RST、FIN、DROP、延迟、抖动、丢包、半开、网线断开、IP 改变、DNS 卡住；记录注入时间并验证检测和恢复状态。Linux netem/nftables 和 Windows 可控代理/测试网关分别跑。
- 数据归属专项：模拟设备输出包含唯一目标/序号的可验证数据，32 线程混合 #1/#2 与 &1/&2 等完整命令；记录命令发送序列和请求结果。注入超时后迟到响应、重连和取消，必须 0 错归/0 错目标/0 二次发布。
- 文件失败注入：磁盘满、无权限、close/fsync 失败、内容变化、校验失败、rename 时断线、源文件变化；验证原目标保持、未知发布状态及清理不覆盖业务错误。
- 真机：每个承诺固件/CPU/OS 验证 read-only 流程及文件临时目录操作。运动、复位、save、PLC/坐标系启停在专门台架按设备操作规程验证，不在运行产线自动执行破坏性压测。
- SDK：C++ RAII/异常、C# Dispose/GC/cancel/Task、Python GIL/asyncio/GC/进程退出，绑定功能矩阵全覆盖。

### 12.2 发布要求

所有正式 target 至少通过构建、ABI、功能、故障恢复、并发与真实硬件烟测；x86/x64、ARM32/ARM64 分别给报告。QEMU 可补功能覆盖，不能替代 ARM 真机性能签收。硬件尚未验收的平台标 experimental，不宣传工业可用。

每版提交测试报告、功能矩阵、ABI 差异、性能基线、长稳曲线、依赖锁/SBOM、许可证、导出符号及运行依赖清单。CI 运行 fmt/clippy、单测/模型测试、交叉编译、ABI/SDK 测试及 cargo audit/deny；安全告警按客户端实际使用路径评估并记录，不盲目忽略。

发现错响应、错目标、双完成、命令重复发送、UAF、数据竞争或静默损坏时立即阻断发布。任何性能改进不能豁免这些门禁。生产日志默认不记录密码、私钥和完整敏感命令；使用 request ID、目标编号、epoch、长度、状态与阶段耗时定位，详细响应只在受控诊断开启。

## 13. 分阶段实施与交付

工期按 1 名熟悉 Rust/SSH/FFI 的核心开发者、可取得各平台与控制器台架估算，约 14–22 人周；这是工作量区间，不是无人值守自动实现承诺。硬件到位、旧固件算法及三种语言发布适配可能延长日历时间。

| 阶段 | 工作量参考 | 工作与交付 | 退出条件 |
| --- | --- | --- | --- |
| P0：事实与可行性 | 1–2 周 | feature-matrix、目标平台/固件表、旧库基线、命令语法/完整目标规则；russh/SFTP 七个 target 最小构建；真机 GPASCII/PTY/算法/多连接验证；保活源码审查 | 所有必需目标有可行路径；全功能清单及可靠性指标可签收 |
| P1：协议与契约 | 1–2 周 | ADR、C ABI 草案、错误/执行确定性、ID 生命周期、内存预算、作用域语法、SDK 约定；golden corpus 与状态机测试 | 所有错误分支及并发顺序有定义；ABI 布局原型通过 |
| P2：核心与 C ABI | 2–3 周 | Runtime/注册表/请求/有界队列/取消/关闭，GPASCII 主路径，类型化电机与坐标系完整命令 | 多线程、关闭竞争、帧边界和错目标专项通过 |
| P3：全业务功能 | 2–3 周 | 状态/数值/正则、Shell、SFTP/原子发布、buffer 加载、维护屏障；C++ 状态位完整迁移 | 除 dpkg 外功能矩阵全部实现并有验证 |
| P4：健康与恢复 | 1–2 周 | 多层探测、事件/快照、epoch、重连、未知结果、rekey/黑洞注入 | 旧响应绝不串入新连接，恢复不重复动作，检测指标有证据 |
| P5：SDK | 2–3 周 | C++17 与 C++98 同步适配、.NET 包、Python 包、完成泵、同步/异步/取消与示例 | 三种 SDK 全功能可调用且结果/错误语义一致 |
| P6：跨平台优化 | 2–4 周 | Windows/Linux x86/x64/ARM32/ARM64 构建发布链、加密后端选择、真机尾延迟与负载优化 | 所有承诺 target 按档位签收，未完成项明确标记 |
| P7：长稳与切换 | 1–3 周 | 72h/7d 长稳、操作规程、打包、部署/回退、试点读流量对照 | 可靠性门禁 0 失败；完整发布证据 |

先产出可运行的最小竖向切片：C 调用连接 → 两个线程提交自包含只读命令 → 请求级结果 → 人工断网 → 两个请求正确终结 → 安全关闭；随后再扩展功能。不能先写一大批 wrapper 再补核心线程模型。

每阶段使用隔离目录/新库名，旧实现保留用于对照与回退。只读查询可做影子比对，但新增查询可能影响控制器负载，应限流；任何写、运动、加载、复位命令禁止双发比较。切换以设备级所有权进行，原客户端退出后再让新库接管，回退前核对设备状态和未知操作。

## 14. P0 必须固定的决策与风险

1. 实际工控机/ARM 板卡清单、Windows/Linux 最低版本、Linux libc、ARM ABI、32 位应用需求及 SDK 运行时版本。
2. 控制器固件、SSH 算法、PTY 要求、GPASCII 作用域语法、SFTP 扩展、多会话上限与 gpascii 并行语义。
3. 必需检测时限与误判容忍、控制命令频率、每设备调用线程数、文件大小及最长维护耗时。
4. 旧正则表达式样本、所有 C++ 状态字段和 Strict/NoThrow 行为对应方式，确保能力不遗漏。
5. 每目标加密后端和性能/体积/构建可行性；旧系统支持与依赖安全补丁之间的取舍必须记录。
6. 默认双 SSH 是否在最小控制器上可承受；不行时选择显式降级档，并写清性能限制。
7. 无法识别完整目标的原始运动命令一律拒绝；需要支持的新语法先加入解析器及测试，不允许绕过可靠性约束。

## 15. 依赖核实来源

以下来源于 2026-11-11 查询，用于可行性与设计依据；最终以 P0 锁定版本的源码、文档及实验为准。

- [russh 官方仓库与加密后端说明](https://github.com/Eugeny/russh)：Tokio 异步 SSH、通道能力及依赖 feature。
- [russh 0.65.1 Config](https://docs.rs/russh/0.65.1/russh/client/struct.Config.html)：保活、nodelay、窗口与背压参数。
- [russh connect_stream](https://docs.rs/russh/0.65.1/russh/client/fn.connect_stream.html)：自建 TCP 流接入。
- [russh Handler](https://docs.rs/russh/0.65.1/russh/client/trait.Handler.html)：主机密钥校验与断连/通道事件。
- [russh-sftp 3.0.1](https://docs.rs/russh-sftp/3.0.1/russh_sftp/)：独立 SFTP 支持及扩展能力。
- [EXT_INFO 无界分配公告](https://github.com/Eugeny/russh/security/advisories/GHSA-4p9f-7p9m-5vfg)及[零最大包长度挂起公告](https://github.com/Eugeny/russh/security/advisories/GHSA-fw5c-8223-6h42)：两项公告列出 0.65 修复，说明版本筛选需要检查实际客户端路径，不能认为内存安全语言自动消除协议级 DoS。
- [Tokio timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html)：取消 Future 不等于撤销远端副作用，非让出执行会影响超时检查。
- [Tokio Runtime shutdown](https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html#method.shutdown_timeout)：阻塞工作可能继续运行，卸载语义需自行管理。
- [Rust 平台支持](https://doc.rust-lang.org/rustc/platform-support.html)：目标与最低系统支持按固定工具链确认。
## 附录 A：异常情况的确定行为

| 情况 | 远端操作规则 | 本地结果与后续 |
| --- | --- | --- |
| 非法目标、缺失目标、非法数值、长度溢出 | 不发送 | INVALID_ARGUMENT，原连接保持可用 |
| 队列满、预算不足 | 不接纳、不发送 | QUEUE_FULL/RESOURCE_LIMIT，明确允许应用稍后重新提交 |
| 排队超时或取消 | 保证该请求不发送 | QUEUE_TIMEOUT/CANCELLED + NOT_SENT |
| 发送途中失败、已发出但响应超时 | 不自动重发 | MAY_HAVE_EXECUTED，退役受影响连接并取消旧代次队列 |
| 收到不完整或归属不明数据 | 不派发下一条命令 | PROTOCOL_ERROR，隔离连接，保留有限诊断数据 |
| 完整帧中为远端错误 | 不将 ACK 当业务成功 | REMOTE_ERROR；确认边界有效才可继续使用通道 |
| 完整帧有效，但类型解析失败 | 不发布默认数值 | PARSE_ERROR，保留原始字节；边界仍有效时连接可继续 |
| 输出缓冲太小 | 不重复执行 | BUFFER_TOO_SMALL + required，可从同一结果重新复制 |
| 调用方 wait 超时，但请求执行预算尚未结束 | 原请求按既定策略继续 | WAIT_PENDING，原 request ID 可继续等待/取消 |
| 请求已经完成后取消 | 不撤回、不重新执行 | 返回已完成状态，保持原终态和原结果 |
| close 与 submit 同时发生 | 按接纳线性化点决定，不悬空 | 已接纳请求按策略终结，之后请求返回 CLOSING |
| 旧事件在新连接建立后到达 | 不影响新连接 | 仅完成旧请求/释放资源，不改写新状态 |
| 文件发布阶段中断 | 不盲目重复发布 | PUBLISH_UNKNOWN，要求查询目标内容/校验值 |
| 核心任务 panic | 停止受影响任务的新业务 | INTERNAL，监督器终结关联请求并隔离状态 |
| shutdown 等待超时 | 不宣布全部结束 | SHUTDOWN_PENDING，保留管理 ID，禁止卸载库 |

P1 将此表细化为每个公开函数的前置条件、后置条件、输出是否修改、连接是否仍可用、是否可能有副作用及可否安全重试，形成接口契约；未列明的新异常先定义行为并增加测试，不能在实现中留给调用方猜测。
## 附录 B：GPASCII 连接池、慢建连与服务上限

根据用户补充，建立 GPASCII 会话可能耗时约 20 秒，且可能因服务上限失败。将其作为正式设计输入；20 秒的具体阶段和原因仍需测量，不能归因于 russh/libssh2 本身，也不能假定改用异步就会缩短远端初始化时间。

### B.1 是否影响已有通信

- 本地实现层：如果持有设备全局锁建连、协调器 await 建连时停止处理已连接 lane、同步认证/文件读取阻塞 worker，20 秒建连会直接拖住已有通信。设计必须消除这类耦合。
- 同 SSH 多通道层：打开一个新通道被拒绝，按 SSH 协议并不要求关闭已有通道；但通道仍共享传输、rekey、协议任务及部分背压。新增请求的处理不能保证对已有传输零影响。参考 [RFC 4254 第 5 节](https://www.rfc-editor.org/rfc/rfc4254.html#section-5)。
- 控制器层：新建 SSH 的密钥交换、认证、进程创建、gpascii 初始化可能竞争 CPU、内存、文件描述符或内部服务资源。服务额度耗尽时固件如何拒绝、等待或回收，不能仅从客户端源码确定。即使独立 TCP 连接也不能隔离这些设备资源。
- 数据正确性层：连接失败不能把某成员的部分响应交给另一成员的请求。所有池成员独立接收、解码和完成，失败只退役受影响成员；设备整体复位或身份异常另按设备级故障处理。

因此：慢建连不应让客户端主动暂停已有通信，但无法无测试承诺控制器侧完全不卡顿。连接池能减少运行期建连次数，不能增加服务硬上限，也不自动增加控制器执行能力。

### B.2 池对象与容量模型

池成员是完整的 GPASCII 会话，不是一个 TCP socket；SSH 已认证不代表 GPASCII 已可用。优先独立 SSH 的 lane 便于故障隔离；同 SSH 多 GPASCII 通道作为显式省资源模式，失败时按实际共享范围退役，不能只删一个池 ID 而留下同 SSH 的其他成员假装健康。

必须分别登记：设备 gpascii 全局上限、SSH 连接/认证上限、单 SSH channel/session 上限、本进程配额、外部工具预留、临时 buffer 加载占用、正在创建成员、关闭后远端尚未确认释放的成员。不能把这些不同额度混为一个 max_connections。

配置上限满足：本库 GPASCII 占用 + 同控制器其他客户端占用 + 维护峰值预留 + 不确定释放占用 ≤ 已验证设备容量。外部客户端无法可靠枚举时，采用运维分配配额及保守预留，不进行扫描式试连“自动探测上限”。多个本机进程分别建池也会耗尽额度；同进程尽量通过共享 Device 对象复用，跨进程靠统一配额或后续集中代理管理。

初始建议：min_ready=1；仅在容量验证后配置 1 个热备或专用读 lane。池不是越大越好。不写死 GPASCII 最大服务数，也不默认 CPU 核数等于池大小。启动时顺序预热，受每设备和全局建连并发令牌约束，单设备默认最多 1 个成员初始化。

### B.3 成员状态和就绪

成员状态：Absent → ConnectingTcp → Handshaking → Authenticating → OpeningChannel → StartingGpascii → Verifying → ReadyIdle ↔ Busy → Retiring → Released。失败进入 Failed/Backoff；远端是否释放不确定时进入 ReleaseUncertain 并保守占额。

池就绪信息分开报告 ready_count、busy_count、creating_count、retiring_count、release_uncertain_count、configured_target、last_create_error。达到 min_ready 可宣告服务可用，同时暴露池容量不足的 degraded 状态；热备失败不把正在工作的 lane 标成 Disconnected。

每成员须独立完成 GPASCII ready、已知无副作用查询及帧边界验证，才可接受业务。尚在创建、只显示提示符、收到部分 banner 或仅 SSH 保活成功都不能进入 Ready。空闲/就绪不代表已有跨 lane 的业务因果顺序。

### B.4 20 秒建连的隔离与预算

建连由独立受监督任务推进，协调器只接收完成/失败事件，不等待它阻塞自己的调度循环；无覆盖网络等待的大锁。DNS/密钥读取/加密计算的阻塞或重计算路径需实测，必要时限流或隔离工作池。

记录 DNS、TCP、SSH KEX、认证、channel open、PTY/Shell 初始化、gpascii 启动和验证各阶段时间。旧代码先等待 /opt/ppmac# 再等 STDIN Open for ASCII Input；20 秒可能包括提示符兼容等待，必须区分“服务繁忙”与“判定条件不适配”，不能一律认定为服务上限。

对这种设备，预热/建连总预算可从 30–60 秒档评估，并保留阶段超时；这不是普通业务命令的超时。普通命令 deadline 从调用时计算，不因等待建连重新计时。池满/无可用 lane 时，仅允许显式且有界的排队，超时返回 NO_READY_LANE/POOL_WAIT_TIMEOUT + NOT_SENT；不暗中开始一个 20 秒建连再让原命令延后执行。

实时运行模式默认不因瞬时并发升高扩容；仅在启动/维护窗口预热。只有应用显式授权的后台补池才可执行，并须限速，且已有通信 p99/队列或控制器负载越过阈值时暂停新的建连尝试。该抑制只能降低后续压力，不能承诺能瞬间撤销已在远端执行的初始化。

### B.5 调度、顺序与池成员复用

每 lane 请求响应仍严格串行。读池仅接收经过分类、可独立读取的请求；电机与坐标系查询同样强制携带完整目标。多个 lane 的读数不是设备同时刻快照，结果附带本地接收时间和 lane/epoch；不得虚构硬件采样时间。

有副作用操作默认在一个设备控制 lane 内保持统一接纳顺序，不因为存在空闲成员就自动并行执行。跨 lane 写后读必须等待写完成并显式建立依赖；同一电机/坐标系的复合操作由设备调度器保持顺序。显式目标消除了隐式选择风险，但不消除并发写入、启动/停止竞争或控制器内部全局状态冲突。

复位、save、buffer 加载等维护屏障覆盖整个设备池，停止相关派发。全局复位后旧成员全部重新验证/重建；不能让其他旧 lane 在新状态中继续发送积压命令。

仅 ReadyIdle、无在途帧、无未决输出且解析状态确定的成员可复用。成员超时后不得直接归还池；先退役并清理。热备接管只处理新请求或已证明 NOT_SENT 的重新提交；绝不重发 MAY_HAVE_EXECUTED 的旧操作。对可能仍在远端执行的写操作，暂停相关目标后续冲突动作，等待应用核对/恢复确认，不能换一条连接就继续依赖原结果的动作。

### B.6 上限失败、远端残留与退避

能从可靠远端错误识别 RESOURCE_SHORTAGE/SERVICE_LIMIT 时按容量故障记录；普通 ready 超时不能武断归类为达到上限。失败成员不影响现有健康成员，不触发全池重连。

容量故障打开“新增成员”熔断器，已有 lane 继续服务；例如从 5/10/30/60 秒冷却档评估，加 jitter，应用明确授权恢复尝试时仅允许一次探测，不因冷却结束自动发起连接。与网络故障重连策略分开，不能套用 100 ms 连续重试造成启动风暴。认证/身份故障停止自动尝试并告警。

客户端取消或 socket 关闭不证明远端 gpascii 已退出。每个新尝试在发起前占容量令牌；关闭后能观察确认释放才归还；无法确认时按固件验证出的策略进入隔离期，不通过盲目重建突破额度。监控 ReleaseUncertain 数量和持续时间，必要时运维清理本应用可证明拥有的会话，不批量 kill 其他客户端 gpascii，也不通过危险命令“腾槽”。若缺少可信的释放确认能力，宁可容量降级并报告，不能宣称配额严格可知。

### B.7 必需新增测试与签收

1. 保持现有 lane 稳定查询，另一个初始化人为延迟 20 秒，验证现有 lane 在本地仍连续处理，记录端到端 p99/p99.9、最大间隔、错误和队列。
2. 人为占满 GPASCII 服务名额，再尝试新增一个成员；验证拒绝/超时仅影响新增成员，现有请求 0 错归、0 错目标，失败后无重试风暴。
3. 独立 SSH 与同 SSH 多通道分别测，区分 SSH 认证负载、gpascii 服务限制和 TCP 队头阻塞；同时观察设备 CPU、内存和进程数。
4. 远端成员创建成功但客户端 ready 超时/断网，验证残留会话计入不确定额度，重复失败不会无界拉起进程。
5. 外部工程工具占用/释放名额、buffer 加载占用临时名额、多个本进程 Client 复用和多进程争用配额分别测量。
6. 一个成员失败时热备接管，注入旧成员迟到响应及未知写结果；不串数据、不重放、不跨过未确认的冲突动作。
7. 对控制器实际发生资源竞争引起的延迟给出证据：若建连期间无法满足运行期指标，该设备档位正式禁用在线扩池，只允许启动或维护窗口补池。

更新 P0/P2/P4/P6 的交付清单：P0 固定服务配额和创建阶段基线；P2 实现池/令牌/设备级有序调度；P4 实现熔断、残留隔离和安全接管；P6 完成上述跨平台/控制器压力试验。整体实施估算另预留约 1–2 人周用于连接池与资源上限实验，若固件会话释放语义复杂则重新评估。
## 附录 C：应用显式管理连接池（替代自动感应扩缩容）

按用户明确要求，应用最了解业务阶段、通信频次和何时可以承担建连开销。正式方案不实现根据排队、利用率或空闲时间自动扩缩容，不根据观测流量推测应用意图。

### C.1 应用与库的职责

应用决定设备何时打开、每类 GPASCII lane 数量、何时预热、是否接受容量不足降级、何时回收，以及故障后是否允许重连和接管。库负责按授权建立会话、限并发和配额、验证 ready、调度请求、隔离坏成员、报告状态及安全关闭。

库可以检测故障并立即禁止使用坏连接，这属于安全保障，不是自动容量策略。指标只提供给应用用于判断或诊断，不触发额外建连。运行期间不自动关闭健康的空闲会话，不自动轮换长寿命会话，不在业务请求路径临时创建连接。

### C.2 建议管理能力

- pool_prepare_async(profile)：显式创建所需角色/数量，附带建连预算、允许的并发数和容量配额；返回管理请求 ID，不阻塞已有数据通路。
- pool_get_state：按角色返回 desired/ready/busy/creating/failed/retiring/release-uncertain，分别标记完整就绪、部分就绪和不可用。
- pool_resize_async(target)：应用显式增减容量；达到硬件/配额上限时拒绝或报告部分完成，不静默修改目标。
- pool_drain_async(members/role)：停止选定成员的新派发，有界等待在途请求，按明确取消策略关闭。
- pool_repair_async(members)：仅修复应用指定的失效成员，并受配额/残留会话/建连并发限制；不隐式重建健康成员。
- pool_set_recovery_policy：默认 Manual；可显式授权“维持既定成员数”的有限自动修复，约束最大尝试、退避和总恢复预算。这不允许增加应用设定目标，也不重放业务请求。

这些为接口族草案，C ABI 与三种 SDK 统一提供。增加/减少容量时创建新的配置 revision；过期管理任务完成后不能覆盖较新的应用目标。缩容期间应用又增容、重连期间应用 close 等竞争必须有确定取消/接纳规则。

### C.3 动态变化的应用使用方式

应用在进入采样/运动阶段前提前准备池并等待业务要求的就绪条件；通信频次改变但既有容量够用时无需任何连接变更。突发由现有 Ready 成员和有界队列承接，超载明确返回 NOT_SENT，不为了处理一个请求临时发起约 20 秒建连。

应用在确定后续不需要某些会话时显式 drain；请求结束仅归还健康成员，不销毁物理连接。应用结束通信时 close，并等待任务停止；远端配额未确认释放则保留不确定记录，不虚报可用额度。

例如：应用启动时准备 1 条有序控制 lane 和 1 条独立读取 lane；批量监测阶段开始前显式增加读取 lane；阶段结束后显式回收额外成员；发生故障则按事件决定继续降级运行、暂停设备操作或修复指定成员。数量只是示例，正式配额由现场确认。

P2 实现确定的管理状态机，而非流量预测器；P6 验证反复 prepare/resize/drain/repair/close 竞争、部分就绪和容量不足。删除前一版自动感应扩缩容的阈值、观察窗和最短寿命算法；保留性能与资源指标，仅用于诊断和应用决策。

## 附录 D：电缆干扰、部分断连与连接池恢复

### D.1 不把丢包、失联与数据损坏混为一类

电缆/链路干扰可能体现为丢包、重传、延迟增加、网卡链路 down/up，或导致会话关闭。客户端不能只根据一次超时就断言物理电缆故障。TCP 提供有序可靠字节流并处理重传，SSH 提供协商算法对应的完整性保护；故障恢复方案仍须按请求 deadline 和协议确认判断有效性，不把收到的一段数据直接当作完整业务结果。

参考 [TCP RFC 9293](https://www.rfc-editor.org/rfc/rfc9293.html) 与 [SSH 传输 RFC 4253](https://www.rfc-editor.org/rfc/rfc4253.html)。使用有完整性保护的算法；MAC/AEAD 校验失败或协议失步必须终止受影响 SSH，不能跳过校验“尽量继续读”。

短时干扰后，若 TCP/SSH 正常恢复、完整响应仍在 deadline 内且业务解析通过，请求正常成功，不因单个包丢失就主动重建。若已发布超时终态，则恢复后迟到的数据不得将其改成成功，也不得用于下一请求。

### D.2 分层隔离故障

| 故障范围 | 连接池动作 | 其他成员 |
| --- | --- | --- |
| 单 GPASCII 通道 EOF/卡住或帧边界丢失 | 从可调度集合摘除；按初版策略退役其所属 SSH，结束关联请求 | 不共享该 SSH 的成员保持独立状态 |
| 一个 SSH/TCP 出错、读写失败、完整性校验失败 | 该 SSH 下全部通道/lane 失效，推进关联 epoch | 独立 SSH 不因这一事件被关闭 |
| 多条独立连接同时异常 | 分别标记状态，发布共同故障相关性告警；不凭相关性伪造故障原因 | 尚能完成有效交互的成员继续按角色资格处理 |
| 本机网卡明确 link down | 标记链路不可用，停止新增派发/建连；受影响在途按预算/取消策略终结 | 不等待无限 TCP 超时；如何退役需一致的已定义策略 |
| 控制器复位、身份改变或应用显式设备级停用 | 按设备级事件阻止派发，旧池重新验证或退役 | 不能把“还有 socket 没报错”当作设备状态仍一致 |

同 SSH 多通道不是独立故障域，不能在 TCP 失效后仍显示同 SSH 另一 lane Ready。反过来，一条独立 SSH 断开也不能触发全部健康 SSH 重连。

系统没有收到其他成员错误，不等于已证明它们健康。成员快照必须有最后有效业务响应时间和当前在途/探测状态；需要确认时在该 lane 空闲帧边界按已配置预算执行无副作用探测。一个 lane 的探测成功不能替另一个 lane 更新健康时间。

### D.3 不完全中断时的服务语义

池传输可用性与设备操作准入分开：

- 完整可用：应用要求的全部角色/成员 Ready，且无阻断操作的未确认结果。
- 降级可用：部分成员失败，仍有符合角色的 Ready 成员；只允许应用事先授权的降级方式。
- 不可用：业务所需角色没有合格 Ready 成员；新请求明确返回无可用 lane，不暗中等待创建 20 秒连接。
- 操作待核对：可能还有健康读取连接，但存在结果未知的写操作，相关电机/坐标系/设备范围禁止继续冲突操作。

如果只损失一个读取 lane，其他读取 lane 可处理尚未绑定成员的新只读请求，前提是应用选择 PoolAnyReady 的路由策略；这只是使用已有容量，不是扩容。显式绑定 lane、持久 Shell 上下文或顺序依赖的请求不能被偷偷迁移。

已接纳但未发送的请求与已发送请求分开：初版对故障成员关联请求显式完成 NOT_SENT 或 MAY_HAVE_EXECUTED；默认不透明迁移旧请求，由应用按结果重新提交。未来如支持未发送请求迁移，需要应用显式许可、保留原 request ID/deadline/顺序，并能严格证明未发送，不能仅以“没收到 ACK”判断。

写 lane 失效不能自动把正在执行的运动命令丢给热备继续。其他 Ready lane 不代表业务流程可继续；应用确认未知操作、授权角色接管后，新控制 lane 才接收新的有序写请求。显式电机/坐标系编号保证目标明确，但不能证明故障前动作未发生。

### D.4 在途请求如何终结

| 故障时请求状态 | 返回语义 | 后续规则 |
| --- | --- | --- |
| 尚未发送且可严格证明 | NOT_SENT + DISCONNECTED/CANCELLED | 应用可根据当前状态重新提交 |
| 发送途中、或已发送未收到完整有效响应 | MAY_HAVE_EXECUTED + 具体传输/超时错误 | 不自动重发；暂停关联冲突动作并核对 |
| 完整有效响应已完成且未超过预算 | 保留已发布结果，不因随后断线改写 | 结果只代表该交互完成，仍不证明运动已结束 |
| 部分响应/半帧 | 协议/传输失败，不发布有效解析值 | 隔离 lane，不能拿另一成员输出补全 |
| 发布结果与超时/故障同时竞争 | 由同一终态仲裁点决定一次 | 迟到事件只清理，不再次通知成功 |

故障事件先在内部关闭派发闸门并确定请求状态，再通知应用，防止事件已报告失败但其他线程仍向旧成员提交。事件包括 member ID、SSH domain、epoch、request ID、原因/可判定程度、受影响角色、最后响应时间和操作确定性。不能在多个线程间依赖“最后一个错误字符串”恢复流程。

对于全局命令或未知原始命令，结果未知的阻断范围取设备级；对能证明仅影响某目标的类型化命令，可按目标和依赖扩大至相关坐标系/电机。不能把电机和坐标系简单当成互不相关资源，P1 定义保守冲突规则。

### D.5 恢复由应用授权

默认 Manual：库只摘除故障成员、终结请求并报告，不自行新建。应用可选择保持剩余连接继续只读、暂停相关业务、显式修复故障成员或关闭整个设备池。

应用若提前授权有限自动修复，只恢复既定成员，不增加容量。每设备最多 1 个恢复尝试，设备范围共享退避/并发预算，避免每条坏连接各自循环 KEX。保留健康连接，不为“统一状态”重建全池；链路恢复通知仅作为允许尝试的信号，不作为 GPASCII ready 或设备业务恢复确认。

新成员必须重新通过主机身份、GPASCII 初始化和无副作用验证，使用新 epoch。恢复传输与恢复业务两件事：禁止重放旧请求，相关操作待核对标记只能由明确恢复流程解除，不能因 ready_count 回升而自动清除。

约 20 秒建立时间意味着没有热备时不能承诺即时恢复；有热备也无法消除同电缆/同控制器的共同故障。物理冗余链路属于额外系统方案，普通连接池不能提供它。

坏连接关闭后远端 gpascii 可能仍占槽；继续计入 ReleaseUncertain，受池配额及服务上限约束。禁止靠持续创建直到成功来修复干扰造成的断连。

### D.6 故障验证与验收

- 定向中断一个 TCP 五元组，其他成员持续读取，验证仅对应故障域退出，健康连接没有被主动重建。
- 丢包/抖动未超 deadline 时正常成功；跨 deadline 后不接受迟到响应，不发生错归。
- 同 SSH 两个通道与独立 SSH 两成员分别注入故障，验证故障域区别。
- 命令写入前、部分写入、远端执行后 ACK 丢失、完整响应发布后分别断网，检查确定性和写操作闸门。
- 网线物理拔插、单向黑洞、链路反复 flap、多条连接先后超时，验证聚合退避和无重连风暴。
- 健康读 lane 可读但写操作结果未知时，验证只读与冲突写准入分离，不因池仍可用就错误继续动作。
- Manual 模式故障后新连接创建次数为 0，除非应用显式修复；授权修复次数及并发不超过配置。
- 恢复 ready、应用 close、缩容及旧事件交错到达，验证 revision/epoch 不允许旧恢复任务把成员重新塞回已关闭池。

所有用例必须在数据归属/目标/重复操作方面 0 错误；同时记录存活成员尾延迟，区分客户端主动阻塞与实际共享链路干扰，不能只统计“连接没断”。
## 附录 E：worker-rearchitecture 实现对 Rust 计划的补充

### E.1 审阅范围与结论

静态阅读 worker-rearchitecture/ 当前工作树（README 标为 2.0.1-dev / ABI 10002），包括 pmac_request_worker.h、核心提交/执行/销毁、Shell 执行确定性、SFTP 边界、worker/fault/transport/concurrency 回归及 REFACTORING.md。该子仓库存在未提交修改，因此应在 P0 固定源码快照与构建标识，不能仅用已有提交号代表本次看到的代码。

结论：有直接价值，可作为请求正确性契约和回归场景来源；不把其 OS 线程、libssh2 续调、公开 handle、兼容层等实现机械翻译为 Rust。它没有实现本计划的显式 GPASCII 池管理和三语言完整 SDK，也不能替代 ARM 和工业长稳验收。本次未运行其测试，文档内历史测试结果均视作原项目记录，不宣称本次复测通过。

### E.2 已落地设计与迁移映射

| worker 代码证据 | 带来的价值 | Rust 实施要求 |
| --- | --- | --- |
| src/pmac_request_worker.h：pmac_job_finish_locked，完成标志在锁内检查 | 超时、取消和执行完成只发布一次 | 统一终态仲裁，结果和执行确定性在同一次同步快照发布 |
| src/pmac_comm_core.c：pmac_shell_before_send，发送尝试与 job->done 共用锁 | 已返回 NOT_STARTED 的请求之后不能发送 | 在协议发送提交前原子仲裁 NOT_SENT 与 MAY_HAVE_EXECUTED；不能仅用 Tokio timeout 包装发送 Future |
| pmac_shell_confirm_completion 与 test_shell_outcome_stages | 完整结束帧已确认但调用方缓冲不足时，远端结果不应标为未知 | 将传输/协议确认、业务结果、API 缓冲/交付状态分开；小缓冲只重取结果 |
| pmac_worker_collect_locked、pmac_worker_wait、test_reclamation | owner 被阻塞时也能回收排队过期/取消项的数量、字节和引用 | Tokio 队列取消后及时释放准入预算；不能等长 I/O 结束后才清队列 |
| collect_locked 的锁内摘链、锁外 release；pmac_job_release 的最终引用交接 | 析构不在调度锁内回调生命周期代码；同步对象不能过早销毁 | Rust Arc/permit/任务所有权明确；Drop 不在持有关键锁时执行可能重入/阻塞的收尾 |
| pmac_owner_call 的 admission、barrier_pending、worker.generation | 连接变更与普通准入隔离，旧任务不跨连接执行 | 池管理 revision、成员 epoch、请求 admission 共享明确的线性化规则 |
| pmac_owner_snapshot、GetConnectionErrorCopy、IsConnected | 请求诊断、连接诊断、本地连接快照分开 | 直接返回不可变请求错误，不沿用最后错误字符串或 TLS 作为异步归属机制 |
| Timed Shell 复制输入/拥有输出/保活引用；兼容同步入口借用参数且等到执行结束 | 提前返回与借用内存不能混用 | 所有异步请求拥有输入输出，SDK wait/cancel/Dispose 后后台不会碰调用方内存 |
| disconnect_impl 与 transport 回归 | 中断传输和释放底层句柄有先后约束，清理期间不能复用失效 socket | 使用 russh/Tokio 所有权及任务 join 管理退出，不把 drop JoinHandle 当成任务已经停止 |
| SFTP 执行中检查控制和服务保活，但不穿插普通事务 | 长操作会占用 lane，单 owner 不自动带来低延迟 | 延续控制/维护隔离；每 lane 顺序执行，跨连接由设备级屏障保持一致性 |
| 电机/坐标系 helper 生成 #n/&n 完整命令 | 既有类型化接口已提供明确目标格式样本 | 迁移所有 helper 并增加并发命令序列断言；原始入口另补强制作用域校验 |

### E.3 对当前 Rust 请求模型的具体补强

1. 增加“调用结果已发布”与“传输已静止/可复用”的独立状态。worker 的 timed waiter 可以先返回，而 payload 由 owner 引用继续持有。Rust 也可能已对应用完成超时，但通道任务仍在退出；池成员此时必须 Quarantined/Retiring，不能因为 Request::completed 就进入 ReadyIdle。只有确认无旧任务/旧帧/待发送数据才可复用或释放其租约。
2. 将发送仲裁覆盖全部操作。worker 的详细进度跟踪集中在 Timed Shell；Rust 必须在 GPASCII、Shell、上传发布、buffer 加载和维护复合操作统一使用，并按步骤记录。不能将“发送失败”一律记 NOT_SENT，也不能将“完整 ACK”泛化为设备动作完成。
3. 队列容量回收需要独立进度。直接把有界 mpsc 的消息标 cancelled，消息仍占其槽位，直到接收任务处理；如果该任务正等长 I/O，就无法满足立即回收。P1 决定可删除队列或独立 dispatcher 的设计，给取消/期限独立推进路径，并同时限制占位节点和有效 payload，防止零字节墓碑无限堆积。
4. 准入预算在大块输入复制/输出预留前取得，失败或取消时按所有权释放。worker 同步兼容入口 input_bytes=0，拥有内存的 Timed Shell 才计入字节预算；新库全部异步请求都拥有内存，必须统一计费，并覆盖执行中、未消费结果与收尾对象。不能把“队列 64 项/1 MiB”当作整个进程内存上限。
5. 批量取消与管理请求分开。worker CancelPending 在连接屏障时拒绝，避免误取消连接操作。Rust 按业务请求/管理请求/关闭信号分类；控制通知具有独立容量，resize/repair/close 的顺序由 revision 明确，不能将所有请求放同一 FIFO 再按名称猜优先级。
6. 同步与异步 SDK 统一 deadline。worker 为兼容旧接口保留“执行超时不含排队”，仅新 Timed Shell 使用总期限。新库没有旧接口兼容义务，统一总执行预算和独立 wait 预算，避免 C++/C#/Python 对同一超时参数解释不同。
7. 保活不复制 owner idle 方案。worker 在空闲/事务边界保活，仍配置 libssh2 want_reply=0；这不构成服务可用确认。Rust 使用独立传输保活与 lane 级业务健康，保留部分池成员故障的隔离，不按一个成员结果修改其他成员时间戳。

### E.4 必须迁移为 Rust 持久回归的场景

- pmac_worker_regression_tests.c 的 test_reclamation / test_concurrent_reclamation：长任务仍被 gate 阻塞时，超时/取消节点已经释放容量并可重新接纳；多等待者引用释放正确。
- pmac_fault_regression_tests.c 的 test_timed_unsent_and_running_control：发送前阻塞并让等待者超时，恢复 gate 后发送次数必须为 0；等待返回后改写调用方缓冲，后台不得再覆盖。
- test_shell_outcome_stages：未连接、完整成功、完整结束后本地输出不足、接收错误、部分发送失败，各自的结果确定性正确。
- test_prompt_control：Shell/GPASCII ready 阶段持续无响应时，应用 close/cancel 不等待整个建连期限；在 20 秒初始化情景下同样覆盖。
- pmac_transport_regression_tests.c：中止/严重 I/O 错误/Disconnect/Destroy 的收尾生命周期。Rust 版改为检查旧任务/流所有权和退出确认，不移植 libssh2 callback 实现。
- test_sftp_eagain_preserves_arguments：迁移其“挂起期间缓冲不被改写、只推进同一事务”的不变量；Rust Future pending/cancel/partial-write 需覆盖，避免把 libssh2 EAGAIN 数值照搬。
- 错误隔离、非有限数值、状态字、分包、文件发布保护、外层解析期间销毁；进一步增加显式 pool revision、部分成员故障和冲突操作闸门。

各测试使用确定的 gate/barrier 控制交错，不依靠固定 sleep 猜测竞态。真实内核/网络测试单独做预算余量，不能要求所有 CI 机器按硬件微秒级时限通过。

### E.5 哪些证据不能直接推广

- 每连接 OS worker 适用于其 libssh2 owner 设计；Rust 保留 lane 独占业务序列，通过共享 Runtime 承载，不默认每 lane 占一个 OS 线程。
- worker 的 10 ms 条件变量片段、不同 I/O 路径 2/10/25 ms 等待上限属于其控制唤醒实现。条件变量/socket 有事件可提前唤醒，不能把上限直接视为每次固定延迟；Rust 需实际比较事件驱动路径的收益。
- 当前并发回归十万次主要使用非法长度/非法缓冲制造并发错误与生命周期压力，不代表十万条有效网络响应已验证归属；新库另需真实有效、带目标和唯一值的混合读写模拟响应测试。
- 现有性能表是约 300 条只读查询的交替样本，且记录注明之后仍有修改。它适合作为基准方法参考，不足以证明 worker 或 russh 全面更快。
- 当前源树、旧候选 SDK、继承的 1.0.40 二进制不是同一版本；P0 固定源码、依赖、产物哈希后再测，不比较来源不明的库。
- REFACTORING 明确长测未完成，部分检测工具无法运行，最新修复的 SDK/在线门禁也有待完成；不能把历史短测记录当成 Rust 计划已经通过的验收。

P0 增加 worker 源码快照和三版本基线；P1/P2 迁移请求契约和 gate 竞态测试；P3 把确定性覆盖至所有业务步骤；P4/P6 将部分池故障与收尾后复用测试设为发布硬门禁。性能优化以这些不变量不被破坏为前提。