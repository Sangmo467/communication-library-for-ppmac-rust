# 功能迁移矩阵

更新于 2026-11-12。**本机已测**指 Windows x64 的自动化/SDK 回归；**本地 SSH/SFTP 已测**指真实网络协议与受控服务端测试；**真机只读已测**仅覆盖列出的 PPMAC 查询。实现或 CI 配置不等于全部平台与固件已经签收。

| 能力 | 新入口/模块 | 当前证据与边界 |
|---|---|---|
| Init/Cleanup/Create/Destroy | `pmac_runtime_*`、`pmac_client_*` | 本机已测；Runtime join、关闭后拒绝准入、子 ID 未释放时 release pending 可重试 |
| 密码/私钥/加密私钥、身份校验 | `pmac_connect_submit` / `pmac_pool_prepare_async` | 本地 SSH 验证密码、密钥及不兜底、known_hosts/固定指纹；真实控制器密码连接已测 |
| TCP 预探测/GPASCII readiness | russh transport | 本地服务分包/banner/ready 验证及真实设备只读连接；旧固件/约20秒初始化仍待专项 |
| GetResponse/原始字节/正则捕获 | `pmac_get_response_async`、`pmac_request_copy_*` | 本地 SSH、模拟和 SDK 已测；同一请求重取不重发；旧三种提取模式32例oracle通过；其他模式为新字节正则，见[迁移说明](pattern-migration.md) |
| 每请求错误/确定性/路由 | request info/error、`pmac_request_get_route` | 本机已测；归属、member/epoch、NOT_SENT/MAY_HAVE_EXECUTED/CONFIRMED |
| Upload/Download | `pmac_upload_submit`、`pmac_download_submit` | 本地 SFTP 已测 staging/校验/权限/取消/发布中断/目标保护；真实磁盘、断电与大文件待验 |
| BufferUpload/Ex | `pmac_buffer_upload_submit` | 上传、EOF 加载、清理及维护屏障已实现；实际控制器程序加载未执行 |
| FileExists/FileRemove | `pmac_file_*_submit` | 本地 SFTP 已测不存在/无权限与删除；未删除用户设备文件 |
| 持久 Shell/隔离 exec | `pmac_shell_submit` | 本地 SSH/POSIX shell 已测 cwd/env、marker、stderr、退出码和取消；真实用户 Shell 工况待验 |
| Abort | `pmac_abort`，`&*adisable` | 命令格式已测；未执行真实设备停止动作 |
| CoordEnable/ExecProg/Hold/Run | `pmac_coord_*` | 格式、目标、严格返回解析已测；真实写/运行操作待验 |
| PLCEnable/PLCDisable | `pmac_plc_*` | 格式与目标已测；未切换真实 PLC |
| MotorKill/Enable/JogStop/AbsJog/IncJog | `pmac_motor_*` | 格式、目标、非有限数值已测；未执行真实运动/停止 |
| CoordStatus/MotorStatus | `pmac_*_status` 与三语言状态值 | 本机解析与冻结旧头90字段对照；Python/.NET逐字段 golden 已测 |
| MotorPos/DesPos/Vel/PosErr | `pmac_motor_*` | 严格有限浮点及命名响应已测；真实电机全接口待验 |
| Servo/Phase 计数与时间 | `pmac_servo_*`、`pmac_phase_*` | 严格解析已测；真机 `Sys.ServoCount/PhaseTime/ServoTime` 短测通过 |
| AbortAll/PLC/Coord/Motor 布尔查询 | 对应 `pmac_*` helper | 严格0/1已测；真实故障/运行状态组合待验 |
| 本地连接状态/显式远端探测 | state snapshot、只读请求 | 本机及真机只读已测；本地快照不发网络包，池部分可用按成员角色判断 |
| Version/ABI/RuntimeInfo | `pmac_version_*`、`pmac_get_runtime_info` | ABI2.1及位宽、元数据本机已测；输入ABI2000仍接受 |
| 显式 GPASCII 池 prepare/resize/drain/repair | `pmac_pool_*` 与三语言 SDK | 已交付；gate回归覆盖慢建连、配额、revision、epoch、取消、关闭、隔离；真机两lane只读短测通过 |
| 线程亲和 GPASCII/显式读池 | 每调用线程固定 member 和独占 Channel；`PMAC_REQUEST_READ_POOL` | 已测同线程顺序、真实多线程并发、open prog 跨线程隔离及拒绝读池副作用；严格P范围分类与8192行分包完整性已测；跨线程/member不提供隐式因果顺序 |
| 维护屏障/结果未知闸门 | core pool、confirm recovery、maintenance repair | 本机gate与本地SSH/SFTP故障已测；ecat reset单次协议确认；$$$***单次超时后重连可读，未重发，save待验 |
| Runtime 广播完成通知 | `pmac_runtime_wait_completion` / `pmac_runtime_wake` | 原生与SDK已测独立cursor、不丢poll→wait通知、退出唤醒；不是消费式结果队列 |
| C++17 / C++98 | `bindings/cpp` | 本机编译/运行；RAII、请求与同步适配；显式状态NoThrow文件存在性、错误名/分类、版本/RuntimeInfo便利层已回归 |
| C# Task/Dispose/CancellationToken | `bindings/dotnet` | 本机原生SDK烟测768同步负测+32异步提交、GC、池、通知和retryable release通过 |
| Python bytes/asyncio/线程/fork | `bindings/python` | 17项unittest及原生SDK同规模短测通过；长期、多进程与跨平台压力不由短测代签 |
| 七个 target 的 CI/ABI/SBOM/候选打包 | `.github/workflows/ci.yml`、`scripts` | 已配置；实际已运行范围见验证状态，未把CI配置计为平台通过 |
| RSA 依赖安全补丁 | `vendor/russh`、补丁完整性脚本 | 最小依赖/借用类型适配已记录；当前锁文件 audit/deny 通过 |
| dpkg 安装 | 按用户范围排除 | 不交付 |
| 72小时/7天长稳 | 按用户本轮范围排除 | 本轮不启动、不作为单独阻断条件 |

完整的旧 C 导出和 C++ 公共方法对应、源快照及剩余语义差异见 [逐符号迁移证据](legacy-symbol-matrix.md)。

## 正确性与发布证据

结果错归、错目标、双完成、重复副作用、UAF、数据竞争或静默损坏均阻断发布。当前自动化和只读短测提供其覆盖场景的证据；其他平台、真实故障注入和完整设备业务仍按 [验证状态](verification.md)、[平台矩阵](platform-matrix.md) 分别签收。用户排除的长稳不会被重新作为本轮门禁。
