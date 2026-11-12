# 旧 C / C++ 公开符号迁移证据

本表按公开声明逐项登记能力，不要求保留旧符号或 ABI。静态映射、模拟测试、本机 SDK 测试与真实设备签收分别记载；出现测试引用不等于该业务已经在真实控制器上验收。dpkg 是唯一排除的业务工作流，底层 libssh2 数值和裸 SSH handle 属于被替换的实现/ABI。

## 冻结依据

- 只读来源：`E:\PMAC\comm`，头文件版本 `1.0.40`，ABI `10001`。没有修改旧仓库。
- 审阅提交：`0ec71531d9ce1b1d42851e54e441b90d8b05d557`。以下 SHA-256 固定本次读取的实际文件，提交号不能单独代替工作树快照。
- 工作树存在改动：`.gitignore` 已修改；`docs/plans/2026-11-11-per-connection-worker-architecture.md`、`docs/plans/2026-11-11-rust-russh-rearchitecture.md` 未跟踪。只登记路径/状态，没有读取用户业务文件。

| 来源文件 | SHA-256 |
|---|---|
| `src/pmac_comm_core.h` | `790DDD9C380612C9E6B70FACB0438B0618BD983C08D5E464F014926428569CF3` |
| `include/pmac_client.hpp` | `4481C052E507ACFC05AB3D6E482F13DA0ADA02E436DA438222198245EA6E972E` |
| `src/pmac_comm_core.c` | `9054ED5647642D9AD04F8EC762C24F5F615935003D786CFE8E717B4A761D869E` |
| `pmac_command_regression_tests.c` | `7650193EFDF39B3C322D34D3C9CA41982AD4E8E5748DE24DDFD3F38F0CAF04EE` |

登记数量：C 公开声明 62 项（含展开后的 ABI guard），C++ 自定义公开方法/构造/转换 176 项（重载按声明行分列），其中状态位/两位字段访问器 90 项。排除 private helper、标准库继承方法和纯 typedef 别名。

## 验证证据索引

| 引用 | 仓库中的测试与实际覆盖 |
|---|---|
| T1 | [FFI 测试](../crates/pmac-ffi/src/tests.rs)：`exported_request_flow_reuses_completed_result`、`runtime_rejects_invalid_configuration_without_leaking_handle`；C / C++ 示例检查 ABI 和生命周期。 |
| T2 | [GPASCII](../crates/pmac-protocol/src/gpascii.rs)：分包、回显/命名值、NAK、尾帧及随机分包；[正则](../crates/pmac-protocol/src/parse.rs)：`regex_preserves_bytes_and_groups`。旧三种提取模式的32例实际C oracle与新regex能力差异见[模式迁移](pattern-migration.md)。 |
| T3 | [本地真实 SSH 服务回归](../crates/pmac-transport-russh/src/tests.rs)：密码、加密私钥、不兜底认证、精确 host/port known_hosts、固定身份、分包 GPASCII；[FFI](../crates/pmac-ffi/src/tests.rs) 验证参数复制。 |
| T4 | [命令](../crates/pmac-protocol/src/command.rs)：`formats_every_legacy_operation_with_explicit_target`、非法目标/NaN/Inf；[解析](../crates/pmac-protocol/src/parse.rs)：完整输入、严格0/1、u64、有限浮点。这些不是每一个 API 的真机读写验收。 |
| T5 | 从冻结的旧 C++ 公共头提取 [90 个 golden 字段](../bindings/python/tests/legacy-status-fields.json)；[Python 逐字段回归](../bindings/python/tests/test_status_migration.py) 与 [.NET smoke](../examples/dotnet/Program.cs) 检查全零/全一/字段值/邻位隔离。C++90字段已静态比较名称、位移和宽度；Rust 单测补最高位/两位字段。 |
| T6 | [真实本地 SFTP 服务](../crates/pmac-transport-russh/src/tests/sftp.rs)：校验、权限、空文件、写失败、发布失败、取消、发布未知及目标保护；buffer 加载核心仍需真实控制器 EOF/程序语法验证。 |
| T7 | [Shell 帧测试](../crates/pmac-protocol/src/shell.rs)：实际 POSIX shell 的分号/注释/cwd；[SSH 回归](../crates/pmac-transport-russh/src/tests.rs)：stdout/stderr、退出码、EOF、持续 Shell 标记隔离。 |
| T8 | [池与生命周期 gate 回归](../crates/pmac-testkit/src/pool_tests.rs)：慢建连不阻塞、独立成员、维护屏障、未知写闸门、revision、epoch、关闭、残留配额及维护故障隔离；[基础请求回归](../crates/pmac-testkit/src/lib.rs)。 |
| T10 | [C++17/C++98 NoThrow 回归](../examples/cpp/no_throw.cpp)：显式status与bool输出分离，不存在/权限失败/格式错误/准入拒绝/异常收尾，以及版本、ABI、RuntimeInfo和错误分类。CMake为两种标准分别编译运行。 |
| T9 | [Python SDK 回归](../bindings/python/tests/test_lifecycle.py)、[Python 原生 smoke](../examples/python/lifecycle.py)、[.NET 原生 smoke](../examples/dotnet/Program.cs)：认证参数、异步 GetResponse、结果归属、只重取不重发、并发/取消/GC、完成通知、池管理与 ABI。原生负测用本地缺失私钥预检，未发设备业务命令。 |

## C 公开声明逐项映射

SDK 列按 C++17 / C# / Python 排列；C++98 同步适配见 `bindings/cpp/pmac_cpp98.hpp`。实现主入口见 [公开头](../include/pmac.h)、[C++](../bindings/cpp/pmac.hpp)、[C#](../bindings/dotnet/Pmac/Client.cs)、[Python](../bindings/python/pmac/core.py)。

| 旧符号（旧头声明行） | 新 C ABI | SDK 对应 | 测试 | 状态/语义 |
|---|---|---|---|---|
| `PMAC_Init` :182 | `pmac_runtime_create` | Runtime constructors | T1/T9 | 生命周期改为显式 Runtime |
| `PMAC_Cleanup` :192 | `pmac_runtime_shutdown / pmac_runtime_release` | Runtime.shutdown / Dispose | T1/T8/T9 | 必须确认 shutdown 完成 |
| `PMAC_CleanupEx` :204 | `pmac_runtime_shutdown / pmac_runtime_release` | Runtime.shutdown / Dispose | T1/T8/T9 | SHUTDOWN_PENDING 可重试 |
| `PMAC_CreateHandle` :214 | `pmac_client_create` | Runtime.createClient / CreateClient / create_client | T1/T9 | 带类型 ID，非裸指针 |
| `PMAC_DestroyHandle` :224 | `pmac_client_close / pmac_client_release` | Client.close/reset / Dispose / close+release | T1/T8/T9 | 关闭与释放分离 |
| `PMAC_DestroyHandleEx` :237 | `pmac_client_close / pmac_client_release` | Client.close/reset / Dispose / close+release | T1/T8/T9 | 显式状态码 |
| `PMAC_Connect` :260 | `pmac_connect_submit` | Client.connect / Connect / connect_async | T3/T9 | 密码/私钥；真实固件仍需验收 |
| `PMAC_ConnectWithProbe` :283 | `pmac_connect_submit` | ConnectOptions.tcpProbe / TcpProbe / tcp_probe | T3/T9 | TCP 预探测参数 |
| `PMAC_ConnectWithKnownHosts` :298 | `pmac_connect_submit` | ConnectOptions.knownHosts / KnownHostsPath / known_hosts | T3/T9 | known_hosts/固定 SHA-256 |
| `PMAC_RequireAbi_10001` :310 | `pmac_abi_version / pmac_get_runtime_info` | Runtime metadata / ABI major check | T1/T9 | 替代旧链接期 ABI guard；不兼容旧 ABI |
| `PMAC_Disconnect` :321 | `pmac_client_close` | Client.close / Close / close | T1/T8/T9 | close 终结该 Client；恢复用 repair 或新 Client |
| `PMAC_GetResponse` :342 | `pmac_get_response_async / pmac_request_copy_response / pmac_request_copy_match` | getResponseAsync / GetResponseAsync / get_response_async | T1/T2/T9 | 同一请求重取；正则语法 G1 |
| `PMAC_FreeRegexResult` :355 | `pmac_request_release` | Request RAII / Dispose / release | T1/T9 | 取消旧 malloc 字符串所有权模型 |
| `PMAC_GetError` :371 | `pmac_request_copy_error` | pmac::Error / PmacException / PmacError | T1/T9 | 每请求不可变诊断，替代 handle 最后错误 |
| `PMAC_GetErrorCopy` :386 | `pmac_request_copy_error` | pmac::Error / PmacException / PmacError | T1/T9 | 小缓冲可重取 |
| `PMAC_GetLibssh2Error` :397 | `pmac_request_copy_error` | Error.status / PmacException.Status / PmacError.status | T1/T3 | 保留分类/文本；libssh2 数值不迁移 |
| `PMAC_Upload` :422 | `pmac_upload_submit` | upload / Upload / upload | T6/T9 | staging/校验/权限；真实磁盘/固件待验 |
| `PMAC_BufferUpload` :443 | `pmac_buffer_upload_submit` | uploadBuffer / BufferUpload / upload_buffer | T6/T8 | 上传+EOF 加载；实际程序加载待真机 |
| `PMAC_BufferUploadEx` :459 | `pmac_buffer_upload_submit` | uploadBuffer(cleanup) / BufferUpload(cleanup) / upload_buffer(cleanup_remote) | T6/T8 | 保留/清理策略；实际加载待真机 |
| `PMAC_Download` :479 | `pmac_download_submit` | download / Download / download | T6/T9 | 本地 staging/替换；真实权限待验 |
| `PMAC_FileExists` :494 | `pmac_file_exists_submit` | fileExists / FileExists / file_exists | T6/T9 | 不存在与无权限分开 |
| `PMAC_FileRemove` :508 | `pmac_file_remove_submit` | removeFile / RemoveFile / remove_file | T6/T9 | SFTP 删除；用户目标删除未上真机 |
| `PMAC_Abort` :520 | `pmac_abort` | abort / Abort / abort | T4 | 命令格式已测；运动/停止未上真机 |
| `PMAC_InstallDpkg` :543 | 明确排除 | EXCLUDED | 范围排除 | 明确排除 dpkg 安装工作流 |
| `PMAC_CoordEnable` :558 | `pmac_coord_enable` | coordEnable / CoordEnable / coord_enable | T4 | 完整坐标目标；写操作真机待验 |
| `PMAC_PLCEnable` :570 | `pmac_plc_enable` | plcEnable / PlcEnable / plc_enable | T4 | 完整 PLC 目标；写操作真机待验 |
| `PMAC_PLCDisable` :582 | `pmac_plc_disable` | plcDisable / PlcDisable / plc_disable | T4 | 完整 PLC 目标；写操作真机待验 |
| `PMAC_CoordStatus` :594 | `pmac_coord_status` | coordStatus / CoordStatus / coord_status | T4/T5 | u64 原值+状态类；不再用零掩盖失败 |
| `PMAC_MotorStatus` :606 | `pmac_motor_status` | motorStatus / MotorStatus / motor_status | T4/T5 | u64 原值+状态类；不再用零掩盖失败 |
| `PMAC_MotorPos` :618 | `pmac_motor_position` | motorPosition / MotorPosition / motor_position | T4 | 严格有限浮点，命名响应已测 |
| `PMAC_ServoErrorCtr` :628 | `pmac_servo_error_count` | servoErrorCount / ServoErrorCount / servo_error_count | T4 | 严格无符号计数 |
| `PMAC_PhaseErrorCtr` :638 | `pmac_phase_error_count` | phaseErrorCount / PhaseErrorCount / phase_error_count | T4 | 严格无符号计数 |
| `PMAC_ServoCount` :648 | `pmac_servo_count` | servoCount / ServoCount / servo_count | T4 | 严格无符号计数 |
| `PMAC_PhaseTime` :658 | `pmac_phase_time` | phaseTime / PhaseTime / phase_time | T4 | 严格有限浮点 |
| `PMAC_ServoTime` :668 | `pmac_servo_time` | servoTime / ServoTime / servo_time | T4 | 严格有限浮点 |
| `PMAC_AbortAllFlagEx` :683 | `pmac_abort_all_flag` | abortAll / AbortAll / abort_all_flag | T4 | 显式状态+严格0/1 |
| `PMAC_AbortAllFlag` :693 | `pmac_abort_all_flag` | abortAll / AbortAll / abort_all_flag | T4 | 旧失败=false 歧义已移除 |
| `PMAC_PLCRunningEx` :709 | `pmac_plc_running` | plcRunning / PlcRunning / plc_running | T4 | 显式状态+严格0/1 |
| `PMAC_PLCRunning` :720 | `pmac_plc_running` | plcRunning / PlcRunning / plc_running | T4 | 旧失败=false 歧义已移除 |
| `PMAC_CoordProgRunningEx` :736 | `pmac_coord_prog_running` | coordProgRunning / CoordProgRunning / coord_prog_running | T4 | 显式状态+严格0/1 |
| `PMAC_CoordProgRunning` :748 | `pmac_coord_prog_running` | coordProgRunning / CoordProgRunning / coord_prog_running | T4 | 旧失败=false 歧义已移除 |
| `PMAC_MotorClosedLoopEx` :765 | `pmac_motor_closed_loop` | motorClosedLoop / MotorClosedLoop / motor_closed_loop | T4 | 显式状态+严格0/1 |
| `PMAC_MotorClosedLoop` :777 | `pmac_motor_closed_loop` | motorClosedLoop / MotorClosedLoop / motor_closed_loop | T4 | 旧失败=false 歧义已移除 |
| `PMAC_CoordMtrEnabledEx` :793 | `pmac_coord_motor_enabled` | coordMotorEnabled / CoordMotorEnabled / coord_motor_enabled | T4 | 显式状态+严格0/1 |
| `PMAC_CoordMtrEnabled` :805 | `pmac_coord_motor_enabled` | coordMotorEnabled / CoordMotorEnabled / coord_motor_enabled | T4 | 旧失败=false 歧义已移除 |
| `PMAC_CoordExecProg` :818 | `pmac_coord_exec_prog` | coordExecProg / CoordExecProg / coord_exec_prog | T4 | 完整目标；程序启动真机待验 |
| `PMAC_MotorKill` :829 | `pmac_motor_kill` | motorKill / MotorKill / motor_kill | T4 | 完整目标；停止操作真机待验 |
| `PMAC_MotorEnable` :840 | `pmac_motor_enable` | motorEnable / MotorEnable / motor_enable | T4 | 完整目标；写操作真机待验 |
| `PMAC_MotorJogStop` :851 | `pmac_motor_jog_stop` | motorJogStop / MotorJogStop / motor_jog_stop | T4 | 完整目标；停止操作真机待验 |
| `PMAC_MotorAbsJog` :863 | `pmac_motor_abs_jog` | motorAbsJog / MotorAbsJog / motor_abs_jog | T4 | NaN/Inf/目标校验；运动真机待验 |
| `PMAC_MotorIncJog` :875 | `pmac_motor_inc_jog` | motorIncJog / MotorIncJog / motor_inc_jog | T4 | NaN/Inf/目标校验；运动真机待验 |
| `PMAC_MotorDesPos` :886 | `pmac_motor_desired_position` | motorDesiredPosition / MotorDesiredPosition / motor_desired_position | T4 | 严格有限浮点 |
| `PMAC_MotorVel` :897 | `pmac_motor_velocity` | motorVelocity / MotorVelocity / motor_velocity | T4 | 严格有限浮点 |
| `PMAC_MotorPosErr` :908 | `pmac_motor_position_error` | motorPositionError / MotorPositionError / motor_position_error | T4 | 严格有限浮点 |
| `PMAC_CoordHold` :919 | `pmac_coord_hold` | coordHold / CoordHold / coord_hold | T4 | 完整坐标目标；写操作真机待验 |
| `PMAC_CoordRun` :930 | `pmac_coord_run` | coordRun / CoordRun / coord_run | T4 | 完整坐标目标；写操作真机待验 |
| `PMAC_ExecuteShellCommand` :980 | `pmac_shell_submit` | shellAsync / ShellAsync / shell_async | T7/T9 | persistent 保留 cwd/env；输出/退出码分开 |
| `PMAC_IsConnected` :990 | `pmac_client_get_state` | isConnected / IsConnected / connected | T8/T9 | 本地状态；池部分可用需看成员角色 |
| `PMAC_VersionString` :992 | `pmac_version_string` | Runtime.version / NativeVersion / native_version | T1/T9 | 库版本字符串 |
| `PMAC_VersionNumber` :993 | `pmac_version_number` | Runtime.versionNumber / NativeVersionNumber / native_version_number | T1/T9 | 库版本数字 |
| `PMAC_AbiVersion` :994 | `pmac_abi_version` | Runtime.abiVersion / NativeAbiVersion / native_abi_version | T1/T9 | ABI major/minor |
| `PMAC_GetRuntimeInfo` :1003 | `pmac_get_runtime_info` | C ABI / Runtime.NativeInfo / Runtime.info | T1/T9 | 构建目标位宽/能力字段 |

## C++ 公开方法逐项映射

同名重载按旧头声明行分别登记；`Strict`/`Raw` 旧命名别名归到同一个严格、有状态码的新业务入口。C++ SDK 默认抛结构化异常，C ABI 提供不抛异常的显式状态；额外便利方法是否存在见 G2。

| 旧公开方法（声明行） | 新入口/SDK | 新 C ABI/数据来源 | 测试/说明 |
|---|---|---|---|
| `MotorStatus::MotorStatus` :72 | pmac::MotorStatus(raw) / Pmac.MotorStatus(raw) / MotorStatus(raw) | `pmac_motor_status` → u64 | T5；显式 raw 构造 |
| `MotorStatus::triggerMove` :76 | pmac::MotorStatus::triggerMove / Pmac.MotorStatus.TriggerMove / MotorStatus.trigger_move | `pmac_motor_status` → u64 | T5；bit 63，width 1 |
| `MotorStatus::homeInProgress` :77 | pmac::MotorStatus::homeInProgress / Pmac.MotorStatus.HomeInProgress / MotorStatus.home_in_progress | `pmac_motor_status` → u64 | T5；bit 62，width 1 |
| `MotorStatus::minusLimit` :78 | pmac::MotorStatus::minusLimit / Pmac.MotorStatus.MinusLimit / MotorStatus.minus_limit | `pmac_motor_status` → u64 | T5；bit 61，width 1 |
| `MotorStatus::plusLimit` :79 | pmac::MotorStatus::plusLimit / Pmac.MotorStatus.PlusLimit / MotorStatus.plus_limit | `pmac_motor_status` → u64 | T5；bit 60，width 1 |
| `MotorStatus::feWarn` :80 | pmac::MotorStatus::feWarn / Pmac.MotorStatus.FeWarn / MotorStatus.fe_warn | `pmac_motor_status` → u64 | T5；bit 59，width 1 |
| `MotorStatus::feFatal` :81 | pmac::MotorStatus::feFatal / Pmac.MotorStatus.FeFatal / MotorStatus.fe_fatal | `pmac_motor_status` → u64 | T5；bit 58，width 1 |
| `MotorStatus::limitStop` :82 | pmac::MotorStatus::limitStop / Pmac.MotorStatus.LimitStop / MotorStatus.limit_stop | `pmac_motor_status` → u64 | T5；bit 57，width 1 |
| `MotorStatus::ampFault` :83 | pmac::MotorStatus::ampFault / Pmac.MotorStatus.AmpFault / MotorStatus.amp_fault | `pmac_motor_status` → u64 | T5；bit 56，width 1 |
| `MotorStatus::softMinusLimit` :84 | pmac::MotorStatus::softMinusLimit / Pmac.MotorStatus.SoftMinusLimit / MotorStatus.soft_minus_limit | `pmac_motor_status` → u64 | T5；bit 55，width 1 |
| `MotorStatus::softPlusLimit` :85 | pmac::MotorStatus::softPlusLimit / Pmac.MotorStatus.SoftPlusLimit / MotorStatus.soft_plus_limit | `pmac_motor_status` → u64 | T5；bit 54，width 1 |
| `MotorStatus::i2tFault` :86 | pmac::MotorStatus::i2tFault / Pmac.MotorStatus.I2tFault / MotorStatus.i2t_fault | `pmac_motor_status` → u64 | T5；bit 53，width 1 |
| `MotorStatus::triggerNotFound` :87 | pmac::MotorStatus::triggerNotFound / Pmac.MotorStatus.TriggerNotFound / MotorStatus.trigger_not_found | `pmac_motor_status` → u64 | T5；bit 52，width 1 |
| `MotorStatus::ampWarn` :88 | pmac::MotorStatus::ampWarn / Pmac.MotorStatus.AmpWarn / MotorStatus.amp_warn | `pmac_motor_status` → u64 | T5；bit 51，width 1 |
| `MotorStatus::encLoss` :89 | pmac::MotorStatus::encLoss / Pmac.MotorStatus.EncLoss / MotorStatus.enc_loss | `pmac_motor_status` → u64 | T5；bit 50，width 1 |
| `MotorStatus::auxFault` :90 | pmac::MotorStatus::auxFault / Pmac.MotorStatus.AuxFault / MotorStatus.aux_fault | `pmac_motor_status` → u64 | T5；bit 49，width 1 |
| `MotorStatus::homeComplete` :94 | pmac::MotorStatus::homeComplete / Pmac.MotorStatus.HomeComplete / MotorStatus.home_complete | `pmac_motor_status` → u64 | T5；bit 47，width 1 |
| `MotorStatus::desVelZero` :95 | pmac::MotorStatus::desVelZero / Pmac.MotorStatus.DesVelZero / MotorStatus.des_vel_zero | `pmac_motor_status` → u64 | T5；bit 46，width 1 |
| `MotorStatus::closedLoop` :96 | pmac::MotorStatus::closedLoop / Pmac.MotorStatus.ClosedLoop / MotorStatus.closed_loop | `pmac_motor_status` → u64 | T5；bit 45，width 1 |
| `MotorStatus::ampEna` :97 | pmac::MotorStatus::ampEna / Pmac.MotorStatus.AmpEna / MotorStatus.amp_ena | `pmac_motor_status` → u64 | T5；bit 44，width 1 |
| `MotorStatus::inPos` :98 | pmac::MotorStatus::inPos / Pmac.MotorStatus.InPos / MotorStatus.in_pos | `pmac_motor_status` → u64 | T5；bit 43，width 1 |
| `MotorStatus::interlockStop` :99 | pmac::MotorStatus::interlockStop / Pmac.MotorStatus.InterlockStop / MotorStatus.interlock_stop | `pmac_motor_status` → u64 | T5；bit 42，width 1 |
| `MotorStatus::blockRequest` :100 | pmac::MotorStatus::blockRequest / Pmac.MotorStatus.BlockRequest / MotorStatus.block_request | `pmac_motor_status` → u64 | T5；bit 41，width 1 |
| `MotorStatus::phaseFound` :101 | pmac::MotorStatus::phaseFound / Pmac.MotorStatus.PhaseFound / MotorStatus.phase_found | `pmac_motor_status` → u64 | T5；bit 40，width 1 |
| `MotorStatus::triggerSpeedSel` :104 | pmac::MotorStatus::triggerSpeedSel / Pmac.MotorStatus.TriggerSpeedSel / MotorStatus.trigger_speed_sel | `pmac_motor_status` → u64 | T5；bit 39，width 1 |
| `MotorStatus::gantryHomed` :105 | pmac::MotorStatus::gantryHomed / Pmac.MotorStatus.GantryHomed / MotorStatus.gantry_homed | `pmac_motor_status` → u64 | T5；bit 38，width 1 |
| `MotorStatus::spindleMotor` :106 | pmac::MotorStatus::spindleMotor / Pmac.MotorStatus.SpindleMotor / MotorStatus.spindle_motor | `pmac_motor_status` → u64 | T5；bit 36，width 2 |
| `MotorStatus::csolve` :110 | pmac::MotorStatus::csolve / Pmac.MotorStatus.Csolve / MotorStatus.csolve | `pmac_motor_status` → u64 | T5；bit 31，width 1 |
| `MotorStatus::softLimit` :111 | pmac::MotorStatus::softLimit / Pmac.MotorStatus.SoftLimit / MotorStatus.soft_limit | `pmac_motor_status` → u64 | T5；bit 30，width 1 |
| `MotorStatus::dacLimit` :112 | pmac::MotorStatus::dacLimit / Pmac.MotorStatus.DacLimit / MotorStatus.dac_limit | `pmac_motor_status` → u64 | T5；bit 29，width 1 |
| `MotorStatus::blDir` :113 | pmac::MotorStatus::blDir / Pmac.MotorStatus.BlDir / MotorStatus.bl_dir | `pmac_motor_status` → u64 | T5；bit 28，width 1 |
| `MotorStatus::softLimitDir` :114 | pmac::MotorStatus::softLimitDir / Pmac.MotorStatus.SoftLimitDir / MotorStatus.soft_limit_dir | `pmac_motor_status` → u64 | T5；bit 27，width 1 |
| `MotorStatus::operator detail::status_word_t` :118 | C++ .raw / C# .Raw / Python .raw | `pmac_motor_status` → u64 | T5；替代隐式转换，保留完整 u64 |
| `CoordStatus::CoordStatus` :130 | pmac::CoordStatus(raw) / Pmac.CoordStatus(raw) / CoordStatus(raw) | `pmac_coord_status` → u64 | T5；显式 raw 构造 |
| `CoordStatus::addedDwellDis` :134 | pmac::CoordStatus::addedDwellDis / Pmac.CoordStatus.AddedDwellDis / CoordStatus.added_dwell_dis | `pmac_coord_status` → u64 | T5；bit 0，width 1 |
| `CoordStatus::sharpCornerStop` :135 | pmac::CoordStatus::sharpCornerStop / Pmac.CoordStatus.SharpCornerStop / CoordStatus.sharp_corner_stop | `pmac_coord_status` → u64 | T5；bit 1，width 1 |
| `CoordStatus::cc3Active` :136 | pmac::CoordStatus::cc3Active / Pmac.CoordStatus.Cc3Active / CoordStatus.cc3_active | `pmac_coord_status` → u64 | T5；bit 2，width 1 |
| `CoordStatus::endDelayActive` :137 | pmac::CoordStatus::endDelayActive / Pmac.CoordStatus.EndDelayActive / CoordStatus.end_delay_active | `pmac_coord_status` → u64 | T5；bit 3，width 1 |
| `CoordStatus::ccMoveType` :138 | pmac::CoordStatus::ccMoveType / Pmac.CoordStatus.CcMoveType / CoordStatus.cc_move_type | `pmac_coord_status` → u64 | T5；bit 4，width 2 |
| `CoordStatus::ccOffReq` :139 | pmac::CoordStatus::ccOffReq / Pmac.CoordStatus.CcOffReq / CoordStatus.cc_off_req | `pmac_coord_status` → u64 | T5；bit 6，width 1 |
| `CoordStatus::ccAddedArc` :140 | pmac::CoordStatus::ccAddedArc / Pmac.CoordStatus.CcAddedArc / CoordStatus.cc_added_arc | `pmac_coord_status` → u64 | T5；bit 7，width 1 |
| `CoordStatus::lookAheadActive` :143 | pmac::CoordStatus::lookAheadActive / Pmac.CoordStatus.LookAheadActive / CoordStatus.look_ahead_active | `pmac_coord_status` → u64 | T5；bit 8，width 1 |
| `CoordStatus::lookAheadFlush` :144 | pmac::CoordStatus::lookAheadFlush / Pmac.CoordStatus.LookAheadFlush / CoordStatus.look_ahead_flush | `pmac_coord_status` → u64 | T5；bit 9，width 1 |
| `CoordStatus::lookAheadReCalc` :145 | pmac::CoordStatus::lookAheadReCalc / Pmac.CoordStatus.LookAheadReCalc / CoordStatus.look_ahead_recalc | `pmac_coord_status` → u64 | T5；bit 10，width 1 |
| `CoordStatus::lookAheadChange` :146 | pmac::CoordStatus::lookAheadChange / Pmac.CoordStatus.LookAheadChange / CoordStatus.look_ahead_change | `pmac_coord_status` → u64 | T5；bit 11，width 1 |
| `CoordStatus::lookAheadStop` :147 | pmac::CoordStatus::lookAheadStop / Pmac.CoordStatus.LookAheadStop / CoordStatus.look_ahead_stop | `pmac_coord_status` → u64 | T5；bit 12，width 1 |
| `CoordStatus::lookAheadDir` :148 | pmac::CoordStatus::lookAheadDir / Pmac.CoordStatus.LookAheadDir / CoordStatus.look_ahead_dir | `pmac_coord_status` → u64 | T5；bit 13，width 1 |
| `CoordStatus::lookAheadLookBack` :149 | pmac::CoordStatus::lookAheadLookBack / Pmac.CoordStatus.LookAheadLookBack / CoordStatus.look_ahead_look_back | `pmac_coord_status` → u64 | T5；bit 14，width 1 |
| `CoordStatus::lookAheadWrap` :150 | pmac::CoordStatus::lookAheadWrap / Pmac.CoordStatus.LookAheadWrap / CoordStatus.look_ahead_wrap | `pmac_coord_status` → u64 | T5；bit 15，width 1 |
| `CoordStatus::segStopReq` :153 | pmac::CoordStatus::segStopReq / Pmac.CoordStatus.SegStopReq / CoordStatus.seg_stop_req | `pmac_coord_status` → u64 | T5；bit 16，width 1 |
| `CoordStatus::segEnabled` :154 | pmac::CoordStatus::segEnabled / Pmac.CoordStatus.SegEnabled / CoordStatus.seg_enabled | `pmac_coord_status` → u64 | T5；bit 17，width 1 |
| `CoordStatus::segMoveDecel` :155 | pmac::CoordStatus::segMoveDecel / Pmac.CoordStatus.SegMoveDecel / CoordStatus.seg_move_decel | `pmac_coord_status` → u64 | T5；bit 18，width 1 |
| `CoordStatus::segMoveAccel` :156 | pmac::CoordStatus::segMoveAccel / Pmac.CoordStatus.SegMoveAccel / CoordStatus.seg_move_accel | `pmac_coord_status` → u64 | T5；bit 19，width 1 |
| `CoordStatus::segMove` :157 | pmac::CoordStatus::segMove / Pmac.CoordStatus.SegMove / CoordStatus.seg_move | `pmac_coord_status` → u64 | T5；bit 20，width 2 |
| `CoordStatus::moveMode` :158 | pmac::CoordStatus::moveMode / Pmac.CoordStatus.MoveMode / CoordStatus.move_mode | `pmac_coord_status` → u64 | T5；bit 22，width 2 |
| `CoordStatus::ccMode` :159 | pmac::CoordStatus::ccMode / Pmac.CoordStatus.CcMode / CoordStatus.cc_mode | `pmac_coord_status` → u64 | T5；bit 24，width 2 |
| `CoordStatus::contMotion` :160 | pmac::CoordStatus::contMotion / Pmac.CoordStatus.ContMotion / CoordStatus.cont_motion | `pmac_coord_status` → u64 | T5；bit 26，width 1 |
| `CoordStatus::blockActive` :161 | pmac::CoordStatus::blockActive / Pmac.CoordStatus.BlockActive / CoordStatus.block_active | `pmac_coord_status` → u64 | T5；bit 27，width 1 |
| `CoordStatus::feedHold` :162 | pmac::CoordStatus::feedHold / Pmac.CoordStatus.FeedHold / CoordStatus.feed_hold | `pmac_coord_status` → u64 | T5；bit 28，width 2 |
| `CoordStatus::segHaltReq` :163 | pmac::CoordStatus::segHaltReq / Pmac.CoordStatus.SegHaltReq / CoordStatus.seg_halt_req | `pmac_coord_status` → u64 | T5；bit 30，width 1 |
| `CoordStatus::csolve` :164 | pmac::CoordStatus::csolve / Pmac.CoordStatus.Csolve / CoordStatus.csolve | `pmac_coord_status` → u64 | T5；bit 31，width 1 |
| `CoordStatus::syncBufferError` :167 | pmac::CoordStatus::syncBufferError / Pmac.CoordStatus.SyncBufferError / CoordStatus.sync_buffer_error | `pmac_coord_status` → u64 | T5；bit 32，width 1 |
| `CoordStatus::bufferError` :168 | pmac::CoordStatus::bufferError / Pmac.CoordStatus.BufferError / CoordStatus.buffer_error | `pmac_coord_status` → u64 | T5；bit 33，width 1 |
| `CoordStatus::linToPvtError` :169 | pmac::CoordStatus::linToPvtError / Pmac.CoordStatus.LinToPvtError / CoordStatus.lin_to_pvt_error | `pmac_coord_status` → u64 | T5；bit 34，width 1 |
| `CoordStatus::pvtError` :170 | pmac::CoordStatus::pvtError / Pmac.CoordStatus.PvtError / CoordStatus.pvt_error | `pmac_coord_status` → u64 | T5；bit 35，width 1 |
| `CoordStatus::runTimeError` :171 | pmac::CoordStatus::runTimeError / Pmac.CoordStatus.RunTimeError / CoordStatus.run_time_error | `pmac_coord_status` → u64 | T5；bit 36，width 1 |
| `CoordStatus::softLimit` :172 | pmac::CoordStatus::softLimit / Pmac.CoordStatus.SoftLimit / CoordStatus.soft_limit | `pmac_coord_status` → u64 | T5；bit 37，width 1 |
| `CoordStatus::radius2Error` :173 | pmac::CoordStatus::radius2Error / Pmac.CoordStatus.Radius2Error / CoordStatus.radius2_error | `pmac_coord_status` → u64 | T5；bit 38，width 1 |
| `CoordStatus::radius1Error` :174 | pmac::CoordStatus::radius1Error / Pmac.CoordStatus.Radius1Error / CoordStatus.radius1_error | `pmac_coord_status` → u64 | T5；bit 39，width 1 |
| `CoordStatus::timersEnabled` :177 | pmac::CoordStatus::timersEnabled / Pmac.CoordStatus.TimersEnabled / CoordStatus.timers_enabled | `pmac_coord_status` → u64 | T5；bit 40，width 1 |
| `CoordStatus::blockRequest` :178 | pmac::CoordStatus::blockRequest / Pmac.CoordStatus.BlockRequest / CoordStatus.block_request | `pmac_coord_status` → u64 | T5；bit 41，width 1 |
| `CoordStatus::interlockStop` :179 | pmac::CoordStatus::interlockStop / Pmac.CoordStatus.InterlockStop / CoordStatus.interlock_stop | `pmac_coord_status` → u64 | T5；bit 42，width 1 |
| `CoordStatus::inPos` :180 | pmac::CoordStatus::inPos / Pmac.CoordStatus.InPos / CoordStatus.in_pos | `pmac_coord_status` → u64 | T5；bit 43，width 1 |
| `CoordStatus::ampEna` :181 | pmac::CoordStatus::ampEna / Pmac.CoordStatus.AmpEna / CoordStatus.amp_ena | `pmac_coord_status` → u64 | T5；bit 44，width 1 |
| `CoordStatus::closedLoop` :182 | pmac::CoordStatus::closedLoop / Pmac.CoordStatus.ClosedLoop / CoordStatus.closed_loop | `pmac_coord_status` → u64 | T5；bit 45，width 1 |
| `CoordStatus::desVelZero` :183 | pmac::CoordStatus::desVelZero / Pmac.CoordStatus.DesVelZero / CoordStatus.des_vel_zero | `pmac_coord_status` → u64 | T5；bit 46，width 1 |
| `CoordStatus::homeComplete` :184 | pmac::CoordStatus::homeComplete / Pmac.CoordStatus.HomeComplete / CoordStatus.home_complete | `pmac_coord_status` → u64 | T5；bit 47，width 1 |
| `CoordStatus::timerEnabled` :187 | pmac::CoordStatus::timerEnabled / Pmac.CoordStatus.TimerEnabled / CoordStatus.timer_enabled | `pmac_coord_status` → u64 | T5；bit 48，width 1 |
| `CoordStatus::auxFault` :188 | pmac::CoordStatus::auxFault / Pmac.CoordStatus.AuxFault / CoordStatus.aux_fault | `pmac_coord_status` → u64 | T5；bit 49，width 1 |
| `CoordStatus::encLoss` :189 | pmac::CoordStatus::encLoss / Pmac.CoordStatus.EncLoss / CoordStatus.enc_loss | `pmac_coord_status` → u64 | T5；bit 50，width 1 |
| `CoordStatus::ampWarn` :190 | pmac::CoordStatus::ampWarn / Pmac.CoordStatus.AmpWarn / CoordStatus.amp_warn | `pmac_coord_status` → u64 | T5；bit 51，width 1 |
| `CoordStatus::triggerNotFound` :191 | pmac::CoordStatus::triggerNotFound / Pmac.CoordStatus.TriggerNotFound / CoordStatus.trigger_not_found | `pmac_coord_status` → u64 | T5；bit 52，width 1 |
| `CoordStatus::i2tFault` :192 | pmac::CoordStatus::i2tFault / Pmac.CoordStatus.I2tFault / CoordStatus.i2t_fault | `pmac_coord_status` → u64 | T5；bit 53，width 1 |
| `CoordStatus::softPlusLimit` :193 | pmac::CoordStatus::softPlusLimit / Pmac.CoordStatus.SoftPlusLimit / CoordStatus.soft_plus_limit | `pmac_coord_status` → u64 | T5；bit 54，width 1 |
| `CoordStatus::softMinusLimit` :194 | pmac::CoordStatus::softMinusLimit / Pmac.CoordStatus.SoftMinusLimit / CoordStatus.soft_minus_limit | `pmac_coord_status` → u64 | T5；bit 55，width 1 |
| `CoordStatus::ampFault` :197 | pmac::CoordStatus::ampFault / Pmac.CoordStatus.AmpFault / CoordStatus.amp_fault | `pmac_coord_status` → u64 | T5；bit 56，width 1 |
| `CoordStatus::limitStop` :198 | pmac::CoordStatus::limitStop / Pmac.CoordStatus.LimitStop / CoordStatus.limit_stop | `pmac_coord_status` → u64 | T5；bit 57，width 1 |
| `CoordStatus::feFatal` :199 | pmac::CoordStatus::feFatal / Pmac.CoordStatus.FeFatal / CoordStatus.fe_fatal | `pmac_coord_status` → u64 | T5；bit 58，width 1 |
| `CoordStatus::feWarn` :200 | pmac::CoordStatus::feWarn / Pmac.CoordStatus.FeWarn / CoordStatus.fe_warn | `pmac_coord_status` → u64 | T5；bit 59，width 1 |
| `CoordStatus::plusLimit` :201 | pmac::CoordStatus::plusLimit / Pmac.CoordStatus.PlusLimit / CoordStatus.plus_limit | `pmac_coord_status` → u64 | T5；bit 60，width 1 |
| `CoordStatus::minusLimit` :202 | pmac::CoordStatus::minusLimit / Pmac.CoordStatus.MinusLimit / CoordStatus.minus_limit | `pmac_coord_status` → u64 | T5；bit 61，width 1 |
| `CoordStatus::homeInProgress` :203 | pmac::CoordStatus::homeInProgress / Pmac.CoordStatus.HomeInProgress / CoordStatus.home_in_progress | `pmac_coord_status` → u64 | T5；bit 62，width 1 |
| `CoordStatus::triggerMove` :204 | pmac::CoordStatus::triggerMove / Pmac.CoordStatus.TriggerMove / CoordStatus.trigger_move | `pmac_coord_status` → u64 | T5；bit 63，width 1 |
| `CoordStatus::operator detail::status_word_t` :207 | C++ .raw / C# .Raw / Python .raw | `pmac_coord_status` → u64 | T5；替代隐式转换，保留完整 u64 |
| `PmacException::PmacException` :219 | pmac::Error / PmacException / PmacError | `pmac_request_copy_error` | T1/T9；请求 ID、状态、执行确定性 |
| `PmacException::errorCode` :226 | Error.status / PmacException.Status / PmacError.status | `pmac_error_info.status` | T1/T9；错误数值体系已重新设计 |
| `PmacException::libssh2Error` :227 | 请求级诊断文本/分类 | `pmac_request_copy_error` | T3；libssh2 特定数值不迁移 |
| `PmacException::errorCodeName` :229 | `pmac::statusName`、`Error::statusName`；C#/Python状态枚举与错误文本 | `pmac_error_info.status` | T10；名称基于新状态码，不复用旧libssh2数值 |
| `PmacException::errorCodeName` :263 | `pmac::statusName`、`Error::statusName`；C#/Python状态枚举与错误文本 | `pmac_error_info.status` | T10；名称基于新状态码，不复用旧libssh2数值 |
| `PmacException::isDpkgError` :267 | 明确排除 | 明确排除 | dpkg 工作流专用判断，不交付 |
| `PmacException::isTransportError` :274 | `pmac::isTransportError` / `Error::isTransportError`；结构化状态分类 | 结构化 `pmac_error_info` | T1/T3/T10；诊断分类不作为重试授权，仍需执行确定性 |
| `PmacException::raise` :288 | throw pmac::Error / throw PmacException / raise PmacError | 无额外网络调用 | T9；直接构造新错误值 |
| `PmacException::raiseLiteral` :295 | throw pmac::Error / throw PmacException / raise PmacError | 无额外网络调用 | T9；直接构造新错误值 |
| `PmacConnectionConfig::PmacConnectionConfig` :323 | ConnectOptions / Pmac.ConnectOptions / connect_async kwargs | `pmac_connect_options` | T3/T9；密码、私钥、公钥路径和主机身份均有对应字段 |
| `PmacRuntime::initialize` :343 | Runtime constructors | `pmac_runtime_create` | T1/T9；显式 Runtime 替代全局引用计数 |
| `PmacRuntime::cleanup` :355 | Runtime.shutdown / Dispose | `pmac_runtime_shutdown / pmac_runtime_release` | T1/T8/T9；显式 Runtime 替代全局引用计数 |
| `PmacRuntime::versionString` :359 | Runtime.version / NativeVersion / native_version | `pmac_version_string` | T1/T9；显式 Runtime 替代全局引用计数 |
| `PmacRuntime::versionNumber` :363 | Runtime.versionNumber / NativeVersionNumber / native_version_number | `pmac_version_number` | T1/T9；显式 Runtime 替代全局引用计数 |
| `PmacClient::PmacClient` :771 | Runtime.createClient / CreateClient / create_client | `pmac_client_create` | T1/T9；带类型 ID，非裸指针 |
| `PmacClient::PmacClient` :778 | Runtime.createClient / CreateClient / create_client | `pmac_client_create` | T1/T9；带类型 ID，非裸指针；新调用分为 createClient + connect，不提供旧自动建连构造签名 |
| `PmacClient::PmacClient` :794 | Runtime.createClient / CreateClient / create_client | `pmac_client_create` | T1/T9；带类型 ID，非裸指针；新调用分为 createClient + connect，不提供旧自动建连构造签名 |
| `PmacClient::PmacClient` :812 | Runtime.createClient / CreateClient / create_client | `pmac_client_create` | T1/T9；带类型 ID，非裸指针；新调用分为 createClient + connect，不提供旧自动建连构造签名 |
| `PmacClient::~PmacClient` :826 | Client.close/reset / Dispose / close+release | `pmac_client_close / pmac_client_release` | T1/T8/T9；关闭与释放分离 |
| `PmacClient::connect` :841 | Client.connect / Connect / connect_async | `pmac_connect_submit` | T3/T9；密码/私钥；真实固件仍需验收 |
| `PmacClient::connect` :861 | Client.connect / Connect / connect_async | `pmac_connect_submit` | T3/T9；密码/私钥；真实固件仍需验收 |
| `PmacClient::connect` :882 | Client.connect / Connect / connect_async | `pmac_connect_submit` | T3/T9；密码/私钥；真实固件仍需验收 |
| `PmacClient::disconnect` :907 | Client.close / Close / close | `pmac_client_close` | T1/T8/T9；close 终结该 Client；恢复用 repair 或新 Client |
| `PmacClient::isConnected` :920 | isConnected / IsConnected / connected | `pmac_client_get_state` | T8/T9；本地状态；池部分可用需看成员角色 |
| `PmacClient::probeConnection` :926 | 显式 command("Sys.ServoCount") / GetResponseAsync / get_response_async | `pmac_get_response_async / pmac_request_copy_response / pmac_request_copy_match` | T2/T3；本地 state 不发包，显式探测使用请求 deadline |
| `PmacClient::getResponse` :936 | getResponseAsync / GetResponseAsync / get_response_async | `pmac_get_response_async / pmac_request_copy_response / pmac_request_copy_match` | T1/T2/T9；同一请求重取；正则语法 G1 |
| `PmacClient::uploadFile` :969 | upload / Upload / upload | `pmac_upload_submit` | T6/T9；staging/校验/权限；真实磁盘/固件待验 |
| `PmacClient::uploadBuffer` :990 | uploadBuffer(cleanup) / BufferUpload(cleanup) / upload_buffer(cleanup_remote) | `pmac_buffer_upload_submit` | T6/T8；保留/清理策略；实际加载待真机 |
| `PmacClient::downloadFile` :1017 | download / Download / download | `pmac_download_submit` | T6/T9；本地 staging/替换；真实权限待验 |
| `PmacClient::fileExists` :1032 | fileExists / FileExists / file_exists | `pmac_file_exists_submit` | T6/T9；不存在与无权限分开 |
| `PmacClient::fileExistsNoThrow` :1052 | C++17/98 fileExistsNoThrow；C#/Python异常式FileExists/file_exists | `pmac_file_exists_submit` | T6/T9/T10；显式status+bool；成功不存在为false，失败保留原输出；每次只提交一次 |
| `PmacClient::removeFile` :1062 | removeFile / RemoveFile / remove_file | `pmac_file_remove_submit` | T6/T9；SFTP 删除；用户目标删除未上真机 |
| `PmacClient::abort` :1076 | abort / Abort / abort | `pmac_abort` | T4；命令格式已测；运动/停止未上真机 |
| `PmacClient::enableCoord` :1084 | coordEnable / CoordEnable / coord_enable | `pmac_coord_enable` | T4；完整坐标目标；写操作真机待验 |
| `PmacClient::enablePLC` :1092 | plcEnable / PlcEnable / plc_enable | `pmac_plc_enable` | T4；完整 PLC 目标；写操作真机待验 |
| `PmacClient::disablePLC` :1100 | plcDisable / PlcDisable / plc_disable | `pmac_plc_disable` | T4；完整 PLC 目标；写操作真机待验 |
| `PmacClient::getCoordStatus` :1108 | coordStatus / CoordStatus / coord_status；C++/C# 将 raw 构造状态值对象 | `pmac_coord_status` | T4/T5；u64 原值+状态类；不再用零掩盖失败 |
| `PmacClient::getCoordStatusStrict` :1115 | coordStatus / CoordStatus / coord_status | `pmac_coord_status` | T4/T5；u64 原值+状态类；不再用零掩盖失败 |
| `PmacClient::getCoordStatusRaw` :1124 | coordStatus / CoordStatus / coord_status | `pmac_coord_status` | T4/T5；u64 原值+状态类；不再用零掩盖失败 |
| `PmacClient::getMotorStatus` :1129 | motorStatus / MotorStatus / motor_status；C++/C# 将 raw 构造状态值对象 | `pmac_motor_status` | T4/T5；u64 原值+状态类；不再用零掩盖失败 |
| `PmacClient::getMotorStatusStrict` :1136 | motorStatus / MotorStatus / motor_status | `pmac_motor_status` | T4/T5；u64 原值+状态类；不再用零掩盖失败 |
| `PmacClient::getMotorStatusRaw` :1145 | motorStatus / MotorStatus / motor_status | `pmac_motor_status` | T4/T5；u64 原值+状态类；不再用零掩盖失败 |
| `PmacClient::getMotorPosition` :1156 | motorPosition / MotorPosition / motor_position | `pmac_motor_position` | T4；严格有限浮点，命名响应已测 |
| `PmacClient::getMotorPositionStrict` :1161 | motorPosition / MotorPosition / motor_position | `pmac_motor_position` | T4；严格有限浮点，命名响应已测 |
| `PmacClient::getServoErrorCtr` :1170 | servoErrorCount / ServoErrorCount / servo_error_count | `pmac_servo_error_count` | T4；严格无符号计数 |
| `PmacClient::getServoErrorCtrStrict` :1175 | servoErrorCount / ServoErrorCount / servo_error_count | `pmac_servo_error_count` | T4；严格无符号计数 |
| `PmacClient::getPhaseErrorCtr` :1180 | phaseErrorCount / PhaseErrorCount / phase_error_count | `pmac_phase_error_count` | T4；严格无符号计数 |
| `PmacClient::getPhaseErrorCtrStrict` :1185 | phaseErrorCount / PhaseErrorCount / phase_error_count | `pmac_phase_error_count` | T4；严格无符号计数 |
| `PmacClient::getServoCount` :1190 | servoCount / ServoCount / servo_count | `pmac_servo_count` | T4；严格无符号计数 |
| `PmacClient::getServoCountStrict` :1195 | servoCount / ServoCount / servo_count | `pmac_servo_count` | T4；严格无符号计数 |
| `PmacClient::getPhaseTime` :1200 | phaseTime / PhaseTime / phase_time | `pmac_phase_time` | T4；严格有限浮点 |
| `PmacClient::getPhaseTimeStrict` :1205 | phaseTime / PhaseTime / phase_time | `pmac_phase_time` | T4；严格有限浮点 |
| `PmacClient::getServoTime` :1210 | servoTime / ServoTime / servo_time | `pmac_servo_time` | T4；严格有限浮点 |
| `PmacClient::getServoTimeStrict` :1215 | servoTime / ServoTime / servo_time | `pmac_servo_time` | T4；严格有限浮点 |
| `PmacClient::isAbortAll` :1220 | abortAll / AbortAll / abort_all_flag | `pmac_abort_all_flag` | T4；显式状态+严格0/1 |
| `PmacClient::isAbortAllStrict` :1225 | abortAll / AbortAll / abort_all_flag | `pmac_abort_all_flag` | T4；显式状态+严格0/1 |
| `PmacClient::isPLCRunning` :1230 | plcRunning / PlcRunning / plc_running | `pmac_plc_running` | T4；显式状态+严格0/1 |
| `PmacClient::isPLCRunningStrict` :1235 | plcRunning / PlcRunning / plc_running | `pmac_plc_running` | T4；显式状态+严格0/1 |
| `PmacClient::isCoordProgRunning` :1244 | coordProgRunning / CoordProgRunning / coord_prog_running | `pmac_coord_prog_running` | T4；显式状态+严格0/1 |
| `PmacClient::isCoordProgRunningStrict` :1249 | coordProgRunning / CoordProgRunning / coord_prog_running | `pmac_coord_prog_running` | T4；显式状态+严格0/1 |
| `PmacClient::isMotorClosedLoop` :1258 | motorClosedLoop / MotorClosedLoop / motor_closed_loop | `pmac_motor_closed_loop` | T4；显式状态+严格0/1 |
| `PmacClient::isMotorClosedLoopStrict` :1263 | motorClosedLoop / MotorClosedLoop / motor_closed_loop | `pmac_motor_closed_loop` | T4；显式状态+严格0/1 |
| `PmacClient::isCoordMotorEnabled` :1280 | coordMotorEnabled / CoordMotorEnabled / coord_motor_enabled | `pmac_coord_motor_enabled` | T4；显式状态+严格0/1 |
| `PmacClient::isCoordMotorEnabledStrict` :1285 | coordMotorEnabled / CoordMotorEnabled / coord_motor_enabled | `pmac_coord_motor_enabled` | T4；显式状态+严格0/1 |
| `PmacClient::execCoordProg` :1294 | coordExecProg / CoordExecProg / coord_exec_prog | `pmac_coord_exec_prog` | T4；完整目标；程序启动真机待验 |
| `PmacClient::killMotor` :1302 | motorKill / MotorKill / motor_kill | `pmac_motor_kill` | T4；完整目标；停止操作真机待验 |
| `PmacClient::enableMotor` :1310 | motorEnable / MotorEnable / motor_enable | `pmac_motor_enable` | T4；完整目标；写操作真机待验 |
| `PmacClient::stopMotorJog` :1318 | motorJogStop / MotorJogStop / motor_jog_stop | `pmac_motor_jog_stop` | T4；完整目标；停止操作真机待验 |
| `PmacClient::jogMotorAbs` :1326 | motorAbsJog / MotorAbsJog / motor_abs_jog | `pmac_motor_abs_jog` | T4；NaN/Inf/目标校验；运动真机待验 |
| `PmacClient::jogMotorInc` :1334 | motorIncJog / MotorIncJog / motor_inc_jog | `pmac_motor_inc_jog` | T4；NaN/Inf/目标校验；运动真机待验 |
| `PmacClient::getMotorDesPos` :1342 | motorDesiredPosition / MotorDesiredPosition / motor_desired_position | `pmac_motor_desired_position` | T4；严格有限浮点 |
| `PmacClient::getMotorDesPosStrict` :1347 | motorDesiredPosition / MotorDesiredPosition / motor_desired_position | `pmac_motor_desired_position` | T4；严格有限浮点 |
| `PmacClient::getMotorVel` :1356 | motorVelocity / MotorVelocity / motor_velocity | `pmac_motor_velocity` | T4；严格有限浮点 |
| `PmacClient::getMotorVelStrict` :1361 | motorVelocity / MotorVelocity / motor_velocity | `pmac_motor_velocity` | T4；严格有限浮点 |
| `PmacClient::getMotorPosErr` :1370 | motorPositionError / MotorPositionError / motor_position_error | `pmac_motor_position_error` | T4；严格有限浮点 |
| `PmacClient::getMotorPosErrStrict` :1375 | motorPositionError / MotorPositionError / motor_position_error | `pmac_motor_position_error` | T4；严格有限浮点 |
| `PmacClient::holdCoord` :1384 | coordHold / CoordHold / coord_hold | `pmac_coord_hold` | T4；完整坐标目标；写操作真机待验 |
| `PmacClient::runCoord` :1392 | coordRun / CoordRun / coord_run | `pmac_coord_run` | T4；完整坐标目标；写操作真机待验 |
| `PmacClient::installDpkg` :1415 | EXCLUDED | 明确排除 | 范围排除；明确排除 dpkg 安装工作流 |
| `PmacClient::installDpkgNoThrow` :1436 | EXCLUDED | 明确排除 | 范围排除；明确排除 dpkg 安装工作流 |
| `PmacClient::getLastError` :1478 | pmac::Error / PmacException / PmacError | `pmac_request_copy_error` | T1/T9；每请求不可变诊断，替代 handle 最后错误 |
| `PmacClient::getRawHandleUnsafe` :1490 | 显式移除裸 SSH handle；新不透明 Client ID | `pmac_client_id` | T1；内部结构/ABI 替换，不是业务能力删减 |

## 公开配置/状态字段与别名

| 旧字段/别名 | 新位置/变化 |
|---|---|
| PmacConnectionConfig.hostname / port / username | `pmac_connect_options.host/port/username`；各 SDK 同语义字段 |
| password / pubkey_path / privkey_path | password / public_key / private_key；私钥口令另有 passphrase；不记录实际凭据 |
| known_hosts_path | known_hosts；旧空字符串关闭校验改为明确的风险选项，不默认接受任意身份 |
| timeout_sec | 新池 connect_timeout_ms，提交总 deadline 毫秒；不沿用旧秒单位/隐式延期 |
| tcp_probe_enabled / tcp_probe_timeout_ms | ConnectOptions 的 TCP probe 开关/毫秒预算 |
| MotorStatus.raw_value / CoordStatus.raw_value | C++ `.raw`、C# `.Raw`、Python `.raw`；全部90字段已登记 |
| PMACException / ConnectionConfig / PMACLibrary / PMACConnection typedef | 对应 Error / ConnectOptions / Runtime / Client；不保留旧别名，不构成新增业务 |

## 迁移核对与剩余设备证据

- **范围排除**：`PMAC_InstallDpkg`、`PmacClient::installDpkg`、`installDpkgNoThrow`、`PmacException::isDpkgError` 以及 dpkg 专用错误/验证。没有生成替代安装流程。
- **G1 已闭环：实际模式语义**。旧GetResponse并未调用正则引擎，只对空模式、`number`、`variable`提取，其他模式原样返回。新实现保留三种提取模式，用冻结旧C helper实际生成32例oracle回归；其他模式提供真实Rust字节正则并明确语法错误。环视/反向引用未曾由旧helper解释，不列作功能删减。见[模式迁移](pattern-migration.md)。
- **G2 已闭环：C++便利层**。C++17/98均提供 `fileExistsNoThrow` 的显式status与bool输出、`statusName`、错误分类/`isTransportError`、版本/ABI/RuntimeInfo。T10覆盖不存在与失败区分、只提交一次和异常释放；不复用旧libssh2数值或把诊断类别当重试规则。
- **G3 已闭环：维护别名分类与模拟恢复**。逐字对照旧C的 `command_requires_extended_timeout`，save/reset及六个EtherCAT别名均已有逐项大小写、空白、复合命令回归。无ACK重新开放输入的reset状态机在受控SSH服务验证；未为修复重发真机reset。真实ecat reset单次Confirmed，`$$$***`单次超时后重连现象分别登记于[维护报告](online-verification-2026-11-12.json)，不把模拟证据改称再次真机确认。新deadline不隐式扩展旧超时下限。
- **G4 真机业务边界**：只读池十条查询、list多行、P1..8192完整响应以及单次维护交互已取得独立报告；这些证据不意味着运动、程序加载、删除用户文件、断电文件发布和每种状态读数已在真实设备验收。见[验证状态](verification.md)。
- **G5 平台/工况边界**：Windows x64与WoW64 x86已实测；其余target的CI配置、最低系统、旧固件算法、外部工程工具会话配额、真实掉线和尾延迟仍按实际证据登记。用户将72小时/7天长稳排除本轮，不作为本轮阻断。

## 维护别名核对明细

| 旧长命令 token | 新入口/分类 | 当前验证边界 |
|---|---|---|
| `save` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
| `$$$` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
| `$$$***` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
| `ecatreset` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
| `ecat reset` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
| `ecatrescan` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
| `ecat rescan` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
| `ecatslaves` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
| `ecat slaves` | `pmac_command_submit` / `Command::raw` 维护分类 | 逐项大小写/空白/复合分类与屏障/受控SSH恢复见T4/T8；真机边界见维护报告 |
