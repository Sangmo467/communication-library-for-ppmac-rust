# 验证状态与后续证据

更新于 2026-11-12。以下区分实现、已执行的本机/受控服务测试、真实控制器调用和仅配置的 CI。用户已将 **72小时/7天长稳排除在本轮任务之外**，本轮不启动，也不把该排除项重新列为单独阻断条件。

## 本轮已执行

客户端环境为 Windows x64、MSVC target、Rust 1.99.1、.NET 8 和 Python 3.12。工作区 MSRV 元数据为1.99；没有宣称更早工具链已验收。

| 范围 | 已取得的证据 | 解释边界 |
|---|---|---|
| Rust workspace | 当前Windows x64为132项、Linux x64为131项全部通过；此前Windows x86和Linux x86/ARM64分别为130/129项通过，ARMv7为128通过/1项既有SFTP时序失败；Windows格式和Clippy通过 | 新增两项为受控MT06/MT08；平台条件测试数量不同，ARM模拟不代替原生验收 |
| GPASCII/严格解析 | ACK/NAK、分包/回显、命名响应、尾帧、超长、完整数值消费、状态字和正则捕获 | 旧实际三种提取模式32例C oracle已回归；其他模式新增字节正则语义，见[模式迁移](pattern-migration.md) |
| 核心/池 gate | 慢建连隔离、严格线程—member/Channel亲和、open prog跨线程隔离、真实多线程并行、排队取消不发送、未知写闸门、维护屏障、revision/epoch、配额及关闭 | MT01/MT06/MT08受控通过，见 `build/reports/mt-controlled-validation.json`；不代替真实控制器资源竞争 |
| 本地真实 SSH | 密码/加密私钥、无密码兜底、host/port known_hosts、固定指纹、GPASCII分包、stderr/退出码和取消隔离 | 本地受控服务器；旧固件算法交集仍需逐固件验证 |
| 本地真实 SFTP | staging、流式校验、权限、不存在/无权限、空文件、写/校验/发布失败、取消、发布未知和原目标保护 | 未在用户控制器执行文件发布、磁盘满/断电或大文件吞吐测试 |
| 原生 C/C++ | C头及C++17/C++98样例编译运行，显式status NoThrow文件查询/错误分类/元数据新增两种标准回归；C/Rust逐字段ABI探针已执行 | 当前完整平台范围以本页平台表和平台矩阵为准 |
| Python SDK | 当前19项unittest在Windows通过（POSIX用例跳过），Linux Python 3.12全19项通过；原生烟测768同步终态负测、32异步提交、pool管理、route、GC/fork、完成cursor和retryable release通过 | 包含8线程512请求归属的FakeNative及真实POSIX fork子进程GC回归；不是长期或跨进程压力签收 |
| .NET SDK | 同规模原生生命周期烟测、认证参数复制、完成准入先于native提交、pool管理、route、GC、通知和retryable release通过 | 使用实际本机原生库，不是仅创建Runtime的旧烟测 |
| 状态位迁移 | 冻结旧C++头90个访问器；Python和.NET对全零/全一/字段值/邻位隔离逐项验证；C++名称/位移/宽度静态对照 | 来源和逐项映射见legacy-symbol-matrix |
| 安全依赖 | 最小russh vendor补丁完整性门禁及当前Cargo.lock的cargo audit/cargo deny通过 | 审计基于具体锁文件与公告库时点，不等于所有运行时行为已经证明安全 |

当前Windows x64/x86 Release各执行6项原生样例/探针并通过，Python和x64 .NET生命周期通过；Linux x64 Release Python/fork与生命周期通过。x86 .NET本轮未重跑。七个Release原生身份与SHA-256统一见 `build/reports/final-native-artifacts.json`，各候选包另有独立manifest。旧汇总中的126/125项计数仅对应当时源码，不作为当前回归计数。

原生完成测试验证了独立等待者不会消费彼此通知、poll与wait之间完成不丢失、`WAIT_PENDING`不改变请求状态。shutdown测试区分了工作线程退出、子ID释放和Runtime最终release；子Request保留时release返回pending，释放后可重试。

## 真实 PPMAC 只读短测

用户授权的设备为 `192.168.0.200`，远端为ARMv7控制器。**库的客户端在Windows x64运行**；远端控制器的CPU架构不构成Linux ARMv7客户端target已经构建/运行的证据。

查询仅为 `Sys.ServoCount`、`Sys.PhaseTime`、`Sys.ServoTime`。驱动检查数值语法、响应携带名称时的名称关联和request ID；本组短测没有执行运动、写入、程序加载或复位。维护操作另见下一节。

| 客户端并发/池 | 实际时段 | 成功/错误 | 本地报告 |
|---|---|---|---|
| 1线程/1lane，约20次每秒 | 30.031秒 | 597 / 0 | `build/reports/soak/20261111T153414Z-1-threads.jsonl` |
| 8线程/2lane，约40次每秒 | 15.141秒 | 600 / 0 | `build/reports/soak/20261111T155830Z-8-threads.jsonl` |

单lane旧报告采用低分辨率计时，因此这里只采纳完成数量、错误数和执行时段，不使用其延迟分位数。两lane报告已使用 `perf_counter_ns()` / QueryPerformanceCounter，报告分辨率为100ns；连接准备共3.922秒，测量在连接完成后开始。

| 两lane样本延迟 | 微秒 |
|---|---|
| p50 | 1778.6 |
| p95 | 3435.7 |
| p99 | 4548.7 |
| 最大值 | 6110.3 |

两lane本轮样本为600次，均落在4096样本容量内。驱动一般使用有界reservoir估计分位数，最大值是全量观测。时段和样本量不足以签收工业p99.9、黑洞检测、长期稳定性或控制器共享负载指标；也未与固定旧库基线作同条件性能比较。

旧路由实现曾完成[显式读池逐条记录](online-read-pool-2026-11-12.jsonl)：用户九条查询及 `list prog100` 全部状态0，路由均为member2/epoch1；`&1P` 返回 `X0 Y0`，list完整230字节多行，`Motor[1].Servo` 的空ACK为控制器实际响应。该记录不冒充当前严格线程亲和版本的联机证据。

`P1..8192` 的原生GetResponseAsync返回状态0、Confirmed，57.7575毫秒，64447字节、8192行，逐项索引无缺失/重复。原始值仅保存在本地忽略工件 `build/reports/online/P1-8192.json`，不复制到文档或fixture；模拟回归另验证严格P范围只读分类与8192行分包完整归属。

现场维护与list的脱敏调用记录另见[维护报告](online-verification-2026-11-12.json)。soak与完整变量值报告为忽略的本地验证工件，候选包通过发布脚本收集对应证据与源码/锁文件哈希。报告不保存密码或私钥路径。

## 已授权维护调用与当前限制

- `ecat reset` 仅提交一次，8.5429566秒返回状态0、执行确定性CONFIRMED，输出包含停止/启动通信及重启堆栈信息。这证明本次协议交互取得确认，不替代EtherCAT所有从站、实时通信或机械状态验收。
- `$$$***` 随后只提交一次，总预算60秒到期返回 `EXEC_TIMEOUT(-5)`，未重发。之后使用新Runtime/Client重连成功，`Sys.ServoCount` 从先前120396062变为70173。较小计数是复位/重启现象的观察，不能把该次超时接口结果改记为协议确认成功。
- 针对无ACK重新开放输入的复位完成状态机已通过受控SSH服务端回归；没有为验证修复再次向真机发送复位。实际新状态机的真机确认仍需新的执行证据。
- 未执行真实电机运动、PLC启停、坐标程序启动/保持/继续、用户文件删除或程序buffer加载。save和其他EtherCAT别名仍需对应设备验收。

## 安全审计与补丁来源

当前通过报告为 `build/reports/security/security.json`，记录 cargo-audit 0.23.0 和 cargo-deny 0.20.2均返回0，创建时间为2026-11-11T15:56:06Z，对应Cargo.lock SHA-256：

```text
22325345ec78e56bbe09fdc376d8411b970916e7a26feb1a7bf9325f0d5a2f67
```

[依赖补丁说明](dependency-patches.md) 固定了上游russh 0.65.1归档来源、90个原始文件的哈希、两个实际修改文件和稳定RSA/PKCS1依赖版本。补丁保留RSA能力，没有用禁用算法或忽略公告替代修复。`scripts/Test-DependencyPatch.ps1`用于约束补丁范围。

`build/reports/security-audit` 中针对旧锁文件的失败报告不代表当前锁文件状态，也不能删除历史失败来冒充首次即通过。SBOM和候选清单必须使用同一锁文件与源码快照；打包/校验工具已实现，跨平台候选仍按实际产物分别签收。

## 平台证据

| 客户端target | 当前已执行范围 | 尚缺范围 |
|---|---|---|
| Windows x64 / x86_64-pc-windows-msvc | 当前130项Rust；Release 6项CTest和Python/.NET生命周期；C/Rust ABI与旧路由真机短测 | 最低OS、当前版本联机、完整故障/负载/业务验收 |
| Windows x86 / i686-pc-windows-msvc | WoW64当前130项Rust、Release 6项CTest、32位Python非法配置1025次回滚通过；本轮未重跑.NET32 | 当前.NET32复测、最低OS、地址空间压力及设备工况/性能签收 |
| Windows ARM64 / aarch64-pc-windows-msvc | 当前Release DLL交叉构建及PE ARM64机器码校验通过；尚未在ARM64主机运行 | 原生ARM64运行、ABI、SDK和设备证据 |
| Linux x64 / x86_64-unknown-linux-gnu | Ubuntu 24.04 WSL当前129项Rust、Python3.12全19项（含真实fork）及Python生命周期通过；历史C/C++/ABI/.NET证据见[Linux SDK记录](linux-sdk-verification.md) | 当前C/C++/.NET复测、物理主机与设备性能、最低内核/系统签收 |
| Linux x86 / i686-unknown-linux-gnu | Ubuntu 18.04 WSL当前129项Rust通过；历史Release C/C++/ABI证据保留 | 当前C/C++复测、独立32位系统、SDK与设备；.NET8 Linux x86路径见平台矩阵 |
| Linux ARMv7 / armv7-unknown-linux-gnueabihf | 当前QEMU 8.2.2为128项Rust通过、1项既有SFTP 250ms时序测试失败；Release hard-float ELF已重建 | 当前全量测试、实际ARMv7板卡、SDK、性能与设备工况 |
| Linux ARM64 / aarch64-unknown-linux-gnu | Ubuntu 24.04交叉工具链与QEMU 8.2.2下当前129项Rust通过，Release ELF已重建；本地未执行C++样例 | 原生客户端板卡/系统、C++、SDK、性能与设备证据 |

ARMv7早期默认并行调度曾出现testkit超时；当前使用串行测试调度。本轮另一个既有SFTP用例 `deadline_during_publish_reports_publish_unknown_without_replaying` 在250ms预算内未进入publish gate，单独重跑仍失败；未把它记为通过，也未为本次GPASCII任务修改该无关测试。

CI矩阵、QEMU限制和SDK运行时约束见[平台矩阵](platform-matrix.md)。配置文件存在、交叉编译成功或QEMU功能通过，都不能代替相应硬件/OS运行与性能签收。

## 本轮仍需明确的证据

1. 最低Windows/Linux/glibc、客户主机/板卡、控制器固件清单、SSH算法交集、PTY需求、SFTP扩展和多会话配额。
2. 约20秒真实初始化、服务名额耗尽/释放、外部工程工具争用，以及控制与维护并行时的设备负载。
3. 各目标RST/FIN/单向与双向DROP、抖动、半开、rekey、迟到帧和关闭交错，及按注入时刻计算的p99检测指标。
4. 真机文件权限/所有者、磁盘满、断线/断电发布、源文件变化和大文件吞吐；实际运动/维护/加载业务的独立台架验收。
5. 同条件旧库/worker/新库基线；1/2/8/32线程、多设备和各平台CPU/RSS/队列/控制器负载下的尾延迟。
6. Loom/Miri/Sanitizer可用范围、持续模糊测试及三语言的更广泛并发/GC/进程退出覆盖。现有短回归不充当这些工具已经运行的证明。
7. 旧公开符号、三种实际提取模式、C++便利层与维护别名迁移已逐项核对，见 [逐符号矩阵](legacy-symbol-matrix.md)。独立业务工程迁移仍按新API语义确认，不把旧helper未解释的任意表达式误称旧引擎能力。

72小时/7天长稳由用户明确排除本轮；本页保留该范围决定，不自动安排或将其缺失计为本轮未完成。
