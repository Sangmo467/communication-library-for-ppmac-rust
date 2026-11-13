# Online Commands 与程序正文透传

命令目录依据 Power PMAC Software Reference Manual，Online Command Specification（手册页 1119–1319）。目录规则见 [online-command-rules.tsv](../crates/pmac-protocol/src/online-command-rules.tsv)；其中记录语法族、作用域、读写/维护分类和代表样例。规则覆盖手册各 Online 命令族，具体参数有效性和设备资源状态由控制器最终判断。

## Online Commands

- 所有命令大小写不敏感；库保留原始文本发送。
- Motor specific 要求同一条命令显式提供 `#` 电机目标；Coordinate-system specific 要求显式提供 `&` 坐标系目标。支持合法的单编号、列表、范围和 `*`。
- 先执行 `#1` 或 `&1` 选择默认目标，不会使后续裸目标相关命令绕过上述校验。例如 `kill` 仍需写为 `#1kill`，`abort` 仍需写为 `&1abort`。
- 手册里的 `k/kill` 为电机命令，因此 `&1k`、`&1kill` 被拒绝；可用 `#1k`、`#1kill` 或相应电机列表形式。
- 额外支持 `&nresume`（以及列表、范围、`&*resume`）作为坐标系控制写入兼容命令；该扩展在规则表中明确标记，不冒充手册 Online 条目。
- `cpx`、`cx` 要求显式坐标系，后续单行程序正文保持原样，包括空格、大小写、引号和分号。整条请求为控制写入，不分析正文中的变量来降低分类。
- `start` 按手册接收可选的非负程序常数，例如 `&1start`、`&1start5`、`&1start 5`；任意正文请使用 `cpx/cx` 或程序缓冲区。

只读和写入按完整语法区分。例：`&1%` 查询、`&1%100` 写入；`&1Q1` 查询、`&1Q1=17.5` 写入；`M0->` 查询定义、`M0->Sys.ServoCount` 修改定义。`rotfree/rotfreeall`、`backup`、`ecat slaves` 为查询。`macro…read` 会复制数据到本地变量，不能按只读请求处理。复杂动态索引无法证明没有副作用时使用控制写入分类。

只有已验证的只读语法可设置 `read_pool`；控制写入和维护命令不能通过只读池准入。正常调用线程和 GPASCII Channel 的亲和规则仍适用。

## 数据结构变量

结构成员采用通用语法，不使用固定结构名前缀白名单。`Sys`、`EncTable[n]`、`Motor[n]`、`ECAT[n]`、`Gate1/2/3[n]`、`Ldata`、`Macro`、`Eip[n]`、`Coord[n]`、`BufIO[n]`、`Gather`、`Tdata[n]`、`UserAlgo` 等均可接完整成员链和数组下标。`Sys.`、`Motor[n].` 是结构前缀示意，本身不是完整的成员查询。

用户声明的合法标量、数组变量名也可直接下发，例如 `MyVariable`、`MyArray[3]`、`MyArray[P1]` 和相应赋值。未在本地验证声明的独立名称查询保守按控制请求路由，原文不改写；这不会把查询变成赋值。包含 `Jog`、`Kill` 或 `System` 的名称不会仅因子串匹配而被拦截。

完整数值下标成员查询和 `.a` 地址查询归为只读；赋值、复合赋值和自增/自减按写入处理。动态下标成员也允许下发，但因表达式可能产生副作用，按控制写入分类，不进入只读池。字段是否存在、可写及取值范围由对应控制器固件判断；本地语法允许并不意味着所有字段都支持写入。

## Program Commands

`GetResponse` / `pmac_command_submit` 自动使用调用线程已确认的程序缓冲区上下文，无需新参数：

```python
client.get_response("open prog 100")  # 等待确认成功
client.get_response("dwell 10")       # 程序正文，同一调用线程
client.get_response("close")          # 单独提交并等待确认
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
