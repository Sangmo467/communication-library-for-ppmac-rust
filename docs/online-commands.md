# Online Commands 与程序正文透传

[中文](online-commands.md) | [English](online-commands.en.md)

命令目录依据 `Power_PMAC_Software_Reference_Manual`，Online Command Specification（手册页 1119–1319）。目录规则见 [online-command-rules.tsv](../crates/pmac-protocol/src/online-command-rules.tsv)；其中记录语法族、作用域、读写/维护分类和代表样例。规则覆盖手册各 Online 命令族，具体参数有效性和设备资源状态由控制器最终判断。

本页的本地分类与程序上下文规则在显式关闭透传时生效。默认透传只保留帧校验；原始 Rust Command::raw 仍是显式规则解析器。

## Online Commands

- 所有命令大小写不敏感；库保留原始文本发送。
- Motor specific 要求同一条命令显式提供 `#` 电机目标；Coordinate-system specific 要求显式提供 `&` 坐标系目标。支持合法的单编号、列表、范围和 `*`。
- 先执行 `#1` 或 `&1` 选择默认目标，不会使后续裸目标相关命令绕过上述校验。例如 `kill` 仍需写为 `#1kill`，`abort` 仍需写为 `&1abort`。
- 手册里的 `k/kill` 为电机命令，因此 `&1k`、`&1kill` 被拒绝；可用 `#1k`、`#1kill` 或相应电机列表形式。
- 按用户要求额外支持 `&nresume`（以及列表、范围、`&*resume`）作为坐标系控制写入兼容命令；该扩展在规则表中明确标记，不冒充手册 Online 条目。
- `cpx`、`cx` 要求显式坐标系，后续单行程序正文保持原样，包括空格、大小写、引号和分号。整条请求为控制写入，不分析正文中的变量来降低分类。
- `start` 按手册接收可选的非负程序常数，例如 `&1start`、`&1start5`、`&1start 5`；任意正文请使用 `cpx/cx` 或程序缓冲区。

只读和写入按完整语法区分。例：`&1%` 查询、`&1%100` 写入；`&1Q1` 查询、`&1Q1=17.5` 写入；`M0->` 查询定义、`M0->Sys.ServoCount` 修改定义。`rotfree/rotfreeall`、`backup`、`ecat slaves` 为查询。`macro…read` 会复制数据到本地变量，不能按只读请求处理。复杂动态索引无法证明没有副作用时使用控制写入分类。

只有已验证的只读语法可设置 `read_pool`；控制写入和维护命令不能通过只读池准入。正常调用线程和 GPASCII Channel 的亲和规则仍适用。

## 数据结构变量

按手册核对的结构根名称和边界识别前缀，匹配不区分大小写；后续成员和多维下标不作白名单检查。完整目录及页码见[数据结构前缀](data-structures.md)。例如 Motor[1].ActPos、CompTable[0].Data[1][2][3]、BrickAC.Chan[0].I2tWarnOnly 均可提交，字段与下标是否合法由控制器判断。

自定义变量支持 StimulusType、StimulusType(11)、MotorState_Mtr(11) 及相应赋值；自定义数组查询不允许 StimulusType[11]。原文保持不变，查询保守使用 Control 调度，不进入只读池。可静态识别的完整数值结构成员查询才按 ReadOnly 处理。

## Modbus 在线命令

同时支持 Power PMAC On-Line Modbus Command Specification 2014-01.pdf 中的 Client 和 Server 在线命令；文件标题页标注 JULY-2015。包括 ModbusAscii、Close/CloseAll/CloseThread、Coil/CoilRead/CoilWrite、Connect、Discrete/DiscreteRead、ExceptionCode、Float/FloatRead/FloatWrite、Holding/HoldingRead、LinuxError、Ping、Query、Register/RegisterRead/RegisterWrite、Response、SocketError、State，以及 ModbusServerLinuxError/Query/Response/State（客户端短名称均带 Modbus 前缀）。

命令名不区分大小写；显式 socket 编号为 0–7。后续参数交给控制器检查，ModbusAscii 的剩余单行 ASCII 正文保持原样，包括分号。Modbus[0].Config.ServerIPAddr[3]、ModbusServer[0].State、Sys.ModbusServerBuffer[0] 同样支持结构访问。这是通过现有 GPASCII 下发 PMAC 的 Modbus 命令，不是新增主机侧 Modbus TCP 传输接口。

ModbusRegisterRead 等 Read 命令会写入本地 PMAC 变量；CloseThread 等具有通信线程语义。因此所有 Modbus 在线命令统一作为 Control 路由到主成员，不能设置 read_pool。为保持 socket 所属上下文，相关命令使用同一个 Client 和调用线程。

调用示例：依次提交 ModbusConnect 0、ModbusRegisterRead 0,10,2,P100、ModbusServerState，并等待各自响应。

## Program Commands

`GetResponse` / `pmac_command_submit` 自动使用调用线程已确认的程序缓冲区上下文，无需新参数：

```python
client.get_response("open prog 100", passthrough=False)  # 等待确认成功
client.get_response("dwell 10", passthrough=False)       # 程序正文，同一调用线程
client.get_response("close", passthrough=False)          # 单独提交并等待确认
```

程序编号由应用选择。每次调用传输一行，沿用原有命令长度限制；NUL、CR/LF 和 ACK/NAK 控制字节仍不允许出现在命令文本中。

- 仅在 `open prog/plc/subprog` 或带显式坐标系的 `open forward/inverse/rotary` 确认成功后进入正文透传。
- 正文不套 Online 动作允许列表，始终按写入处理。`system`、`pread`、`dread` 以及其他 Program Commands 可在该上下文中透传，由控制器编译/解释；它们不因此成为独立 Online 命令。
- 上下文绑定到原调用线程、成员和连接 epoch。其他线程不能继承；断线、成员修复或连接重建使旧上下文失效。
- `close` 必须单独提交。关闭已接纳时停止接收后续正文，避免已退出程序缓冲区后把裸程序命令当成 Online 命令执行。
- 正文执行错误会使本地程序上下文失效并退役对应成员；库不自动重发程序行。应用应根据错误确定性和设备状态决定恢复方式。
- 全局 `close all buffers` 使用维护屏障，并清除本 Client 的所有程序上下文。

Rust 直接调用方使用 `Client::parse_command_for_current_thread(text)` 后提交 `Operation::Command`。`Command::raw` 只解析 Online Commands；`Command::program_line` 是有标记的正文构造器，Client 在准入和发送前仍会检查有效缓冲区上下文。

## 回归覆盖

[命令样例集](../crates/pmac-protocol/tests/online-command-corpus.tsv) 与规则表样例分别覆盖大小写、读写分类、显式作用域及原文保留。FFI 测试通过受控 Transport 验证正常准入与只读池拒绝写入。核心和本地 SSH 测试覆盖同线程正文、跨线程隔离、关闭排队、取消、修复以及全局缓冲区关闭。

这些验证证明本地接收、分类和路由行为；并不代表已在真实控制器上逐条执行运动、复位或修改配置。

## 跨 Client 全局关闭与控制器错误

`close all buffers` 会影响其他 Client 或外部程序打开的缓冲区。其他 GPASCII 通道即使本地仍记录 `open prog`，下一次提交也可能收到：

```text
stdin:1:1: error #31: Buffer closed due to another thread:
```

GPASCII 通道初始化时必须取得 PTY，使控制器合并诊断与 stdout，并在 ACK 响应前刷新错误；PTY 请求失败会中止初始化，不退回可能延迟 stderr 的无 PTY 模式。GPASCII 的同步结果由 stdout 响应帧决定：NAK 或 stdout 中的标准 `error #<编号>` 诊断返回 `REMOTE_ERROR`；ACK 到达即完成请求，不等待也不根据 SSH ExtendedData（stderr）改判结果。stderr 是异步诊断；请求完成前收到且 `stdin` 行号匹配当前命令的诊断才保留在该请求的 `stderr` 字段。属于较早命令的延迟诊断仅记录警告日志，不会缓存或附加到后续请求；无行号、无法归属的诊断同样仅记录日志。诊断独立限长，超出部分丢弃并记录警告。stdout 错误详情保留原始命令、响应正文及已接收诊断。同步 C 调用须立即使用 `pmac_copy_last_error` 读取，异步请求使用 `pmac_request_copy_error` 并保留 Request ID 至复制完成；C++/Python/.NET 绑定通过其错误/异常携带诊断。`CONFIRMED` 只说明完整协议响应已收到，不表示语句执行成功。

stdout 返回 error #31 后，库使受影响的旧程序上下文失效并退役对应成员。不会自动补发 `close`，不会自动重发原语句，更不会把恢复或清理结果改写成原请求成功。应用必须读取原始错误，再按池恢复接口修复或重连，并在确认设备状态后显式重新打开需要的程序缓冲区。成员可能被多个调用线程共享；这些线程会观察到旧 epoch 不可用，需要协同恢复。其他独立 Client 不会被库主动关闭，但全局命令对控制器的影响仍然存在。

同一 Client 通过库发出的 `close all buffers` 会在维护屏障后清除已知本地程序上下文；旧正文可能在本地准入时被拒绝，合法在线命令也仍可能读到远端通道的延迟 error #31。该诊断位于 stdout 时同步返回错误；位于异步 stderr 时仅作为诊断交付，不触发失败或成员退役，应用须另行核对程序缓冲区状态。每个 Client 的在线 GPASCII 通道总额度为 16，包含预建及释放不确定的占用；不得通过新线程或池修复绕过未确认释放的计数。

GetResponse/command 默认开启透传：跳过 online/program 分类及本地程序上下文检查，原文逐行发送。显式关闭透传可启用规则校验：C 使用 PMAC_REQUEST_CLASSIFY（8），C++ 使用 passthrough=false，C# 使用 Passthrough:false，Python 使用 passthrough=False，MATLAB 使用 struct('passthrough', false)。只读池请求自动启用分类；C 显式 PASSTHROUGH（4）不可与 READ_POOL 或 CLASSIFY 组合。基础帧校验和远端 error # 上报始终生效。

## 程序缓冲区 jog / jogret

确认打开程序缓冲区后，支持逐行原样提交 jog / jogret 正文，包括列表与范围、星号、P/Q 表达式、方向、返回位置和尾部分号。例如 jog1..3,5..7:0;、jog=10000^-50;、jogret25..27=*、jog+ 1..5,8、jog-1,2,3;。这些行不按独立 Online 命令重新分类，也不需要请求级 PASSTHROUGH 标志；仍须使用同一 Client、调用线程和已确认的缓冲区。模拟测试验证上传原文和成员亲和，实际编译与执行合法性由控制器决定。
