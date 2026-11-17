# 2.0.4 当前代码复核：缺陷与性能机会

日期：2026-11-15。范围为当前工作区，而非仅远端 tag 的干净快照；工作区包含既有未提交改动。使用 GitNexus query/context 定位，再以当前源码和离线探针验证。没有修改业务实现、连接控制器、提交或推送。报告及忽略目录内探针通过 PowerShell 写入 UTF-8 无 BOM。

本轮列出 11 项正确性/资源问题与 5 组性能机会。未发现足以确认的 P0/P1；P2 表示需要修复的中优先级问题，P3 为较低优先级。静态确定路径与设备实测严格分开；性能建议不等于已证明能提高端到端吞吐。

## 正确性与资源问题

### 1. P2：Python Client 自动回收可能永久遗留原生句柄（离线复现）

位置：bindings/python/pmac/core.py:453–457；bindings/dotnet/Pmac/Client.cs:146。

Python finalizer 执行 close(wait=0) 后立刻 release，忽略两次返回。关闭在后台完成，release 若返回 SHUTDOWN_PENDING，Python 对象此时已经消失；Runtime 只有 WeakSet，无法再取得这个 Client 的原生 ID 以重试释放。

离线真实 DLL 实验：create_client → del → gc.collect → 等待 0.3 秒，原生 ID 仍有效且状态为 Closed；Runtime.shutdown 返回 -14；手工 release 这个 ID 返回 0 后 shutdown 成功。该问题无需连接设备。显式 with/close 是推荐用法，但一个未连接对象的 GC 不应让后续显式 Runtime.shutdown 无法完成。

.NET 已追加真实 DLL 离线复现：编译当前全部 C# 源码，通过 NoInlining 工厂创建 Client 并仅保留 WeakReference 与反射取得的 ID；GC 后 client_alive=False，Runtime.Shutdown 返回 -14，手工 pmac_client_release(id)=0 后再次 Shutdown 成功。因此 C# 同源问题也已实测确认，位置 Client.cs:146、Runtime.cs:10、103。实验目录 target/review-csharp-gc。

建议：由 Runtime 独立保管仍未释放的原生 ID，或交由可重试清理监督任务；不要仅延长 finalizer 阻塞时间。回归应检查 GC 后原生 ID 被回收、Runtime 可显式关闭。

### 2. P2：Gather 的 64 MiB 队列预算实际可以接近翻倍（分配模式实测）

位置：crates/pmac-ffi/src/gather.rs:92、685、708、722。

配置检查按 rows × batches × items × 8 计费，但接收端从空 Vec 逐行 extend，几何扩容的额外 capacity 随 mem::take 一起进入队列。容量超额不是少量结构体开销。

合法配置 items=128、batchRows=2049、queueBatches=31，预算 65,043,456 B；当前 Rust 独立实验每批 len=262272、capacity=524288 个 double，队列实际分配 130,023,424 B，另有当前批次和回调批次。

建议精确预分配批次容量、交付 Box<[f64]>，或按实际 capacity 计费。测试应覆盖非二次幂 batchRows 和 items，而不只检查逻辑长度。

### 3. P2：geterrors 也能突破声明的队列内存预算（分配模式实测）

位置：crates/pmac-ffi/src/geterrors.rs:49、317、350；docs/geterrors.md:328。

逐字节 push 的行 Vec 几何扩容，mem::take 保留超额 capacity。max_line_bytes=65537、queue_lines=1023 的合法配置预算 67,044,351 B，当前 Rust 实测每行 capacity=131072，队列实际 134,086,656 B。该实验验证分配增长模式，不代表已经在设备上积压了满队列。

建议交付精确大小 slice，或对真实分配计费；行长边界与队列深度组合需增加覆盖。实验文件：target/gather-live/vec_capacity.rs。

### 4. P2：Prepare 配额不足后残留半初始化状态（静态路径确定）

位置：crates/pmac-core/src/client.rs:1046–1059；crates/pmac-core/src/pool.rs:161–190。

prepare 先把 options 写入 PoolState，再调用 set_profile。进程级 256 成员预留配额不足时 set_profile 返回 ResourceLimit，但 options 不回滚。其他 Client 释放配额后，再次普通 Connect 仍会被 “pool is already prepared” 拒绝。此时用户必须改用 PoolResize 或重建 Client，而不是正常重试连接。

建议先成功预留 quota 再发布 options，或完整回滚失败状态。回归场景：占满进程配额 → 新 Client Connect 失败 → 释放部分配额 → 同一 Client Connect 可成功。尚未跑容量耗尽的动态复现。

### 5. P2：被新 revision 取代的连接即使关闭成功也登记为 uncertain（静态路径确定）

位置：crates/pmac-core/src/client.rs:1227–1229。

create_member 成功拿到 Session，但 check_revision 失败时，忽略 session.close().await 的结果，无条件把 (id, epoch) 放入 uncertain。确认关闭成功和关闭结果未知被合并，后续配额计算会计入虚假占用。并发管理操作取消或替代连接过程时可能触发，不是每次普通 Connect 都会出现。

建议根据 close 结果区分；成功时回收成员和配额，未确认时保留 uncertain。回归应在延迟连接的同时提交替代管理操作，并断言成功关闭不留下 uncertain。
### 6. P2：长远端路径上传在本地拒绝发布，却报告 PublishUnknown（静态路径确定）

位置：crates/pmac-transport-russh/src/files.rs:103–130；session.rs:213–218。

路径验证未限制长度。完整上传并校验后，程序构造含 staging 和 remote 两个路径的 /bin/mv 命令，并提前 mark_publish_started。shell 对超过 1023 字节的命令本地返回 InvalidArgument；上传代码却将所有错误包装为 PublishUnknown.sent().fatal()。合法长路径或含大量单引号、经 shell_quote 膨胀的路径可触发。

后果：mv 确定未发送，却报告发布结果未知并留下 staging、触发恢复流程；还浪费已经完成的上传和校验。建议提前验证生成命令长度，或使用可用的 SFTP 原子 rename；发布状态应贴近真正发送点。没有进行真实长路径上传实验。

### 7. P2：部分 C ABI 业务函数参数失败时保留上一次错误信息（离线复现）

位置：crates/pmac-ffi/src/business.rs:271–279，类似分支还在 253、266、292、305、318 等处。

构造命令失败时直接返回 error.code，未经过 ffi_status，因此没有清空/更新线程局部 LAST_ERROR。同一线程先产生另一个 InvalidArgument，再调用 pmac_motor_position(..., motor=-1)，返回 -1，但 pmac_copy_last_error 仍给出前一个错误。

真实 DLL 无设备探针输出：seed_status=-1，before='input structure is shorter than the required ABI layout'，motor_status=-1，after 仍为同一句，而非非法电机编号。C# Runtime.Check 和 Python _check_business 按状态码读诊断时会误用这段旧信息。其他 C/C++ 调用者若读 last_error 也受影响。

建议统一在最外层 ffi_status 内做参数/命令构造。回归必须连续产生两次相同状态码但不同原因的错误。探针：target/audit-20261115/stale_error.py。

### 8. P3：公开 ShellDecoder 的完成分支漏查最终输出长度（离线复现，传输层有补充保护）

位置：crates/pmac-protocol/src/shell.rs:157–172。

未发现结束标记时会检查累计 stdout.len；发现结束标记时却直接追加并返回 Complete。直接调用公开协议 crate，maximum=1、payload=20，完整单包帧被接受，返回 20 字节 stdout。分片方式会影响是否提前失败。

内置持久 Shell 传输在 session.rs:534 另有 stdout+stderr 总长度检查，因此不能把此项描述为所有高层 Shell API 都能绕过预算。建议协议解码器自身仍统一执行最终长度检查，并测试所有分包边界。探针：target/audit-20261115/src/main.rs。

### 9. P2 条件风险：Gather 清理可能吞掉 killall 失败并提前解锁

位置：crates/pmac-ffi/src/gather.rs:584。

清理命令将 killall gather nc 的所有错误通过 || true 丢弃，随后移除 FIFO、owner 和锁目录。权限错误、命令不可用或进程尚未退出时，只要后续删除成功，shell 仍返回成功。unlink FIFO 路径不等于关闭活进程已打开的文件描述符。

这条错误掩盖路径在代码上确定，但本轮没有在设备上模拟 killall 失败；不能将“有残留进程”当作已观测事实。建议跟踪本会话 PID，区分不存在与终止失败，并确认进程退出后解锁。文档明确约定 gather/nc 独占，所以全局 killall 影响其他软件不另算一个新缺陷。

### 10. P3：DART 预算表永久保留失效 Weak 和主机字符串

位置：crates/pmac-transport-russh/src/dart.rs:25–40、76–79。

全局 HashMap<String, Weak<DartBudget>> 没有删除失效项。大量不同主机的连接尝试（包括失败）会持续增加 key；预算对象析构只释放 Arc，不删除表项。固定少量设备的常规使用影响较小，设备发现或扫描型应用更容易积累。

建议插入时周期清理 dead Weak，或加容量上限。此项为静态确认，未进行百万主机内存压力实验。

## 性能机会

### 11. 高优先级：GPASCII 分片解码累计重复扫描，呈平方级增长（微基准）

位置：crates/pmac-protocol/src/gpascii.rs:49–79。

每次 push 都从 buffer 起点查 ACK/NAK；未找到时还扫描前导空白。长响应被大量小包切分时，已检查字节会反复扫描。Release 独立探针，128 字节分片，每档 10 次平均：64 KiB=6481 微秒，128 KiB=25077 微秒，256 KiB=88732 微秒。数据翻倍耗时约增加 3.5–3.9 倍，符合 O(n²) 风险。该结果是本机解析微基准，不等于真实网络延迟或发布构建吞吐。

建议保存扫描游标，仅扫描新增字节，或按帧状态增量处理；同时保持 echo、前导空白和最大长度判断不受分片影响。

### 12. 高优先级：正则提取把所有捕获组复制后才选择一组

位置：crates/pmac-protocol/src/parse.rs:139–147；crates/pmac-ffi/src/lib.rs:762–778。

每次调用 Regex::new，然后对完整匹配和全部 captures 分别分配 Vec。C ABI 实际通常只要一个 group，而且“先查询长度、再复制”的惯用调用会再执行一次编译、扫描和分配。

微实验：4096 字节响应、64 个嵌套捕获组，复制 payload 合计 266240 字节，是输入的 65 倍。组数与响应长度放大时会产生可观瞬时内存；不能只依据响应 retained budget 估计峰值。

建议内部用范围/借用切片表达结果，只复制被选中的组；正则编译可做有界缓存或提供编译句柄，不能引入无限缓存。

### 13. 中高优先级：每个请求完成都全表扫描

位置：crates/pmac-core/src/client.rs:1538–1542。

ActiveTask::drop 对 requests HashMap retain，逐项 upgrade Weak 并锁 Request 获取状态。N 个并发请求完成时累计可达 O(N²)；持有请求表锁还会干扰 submit、snapshot、close。

建议 ActiveTask 保存自身 request_id，完成时直接 remove；其他死亡 Weak 做阈值/周期清理。此项未做端到端基准，应按 64/256/1024 并发测量后决定收益。

### 14. 中优先级：Gather/geterrors 高频路径分配与拷贝

位置：gather.rs:636、651–653、685、722；geterrors.rs:317、323–325。

Gather 数值行也先 to_ascii_lowercase 分配，再解析到每行 Vec，之后复制到批次 Vec；每批 mem::take 又丢掉接收端容量。geterrors 每行 mem::take，连忽略的空行/ACK 行也释放容量。

建议数字行快路径、无分配 ASCII 诊断匹配、直接写预分配批次；忽略行仅 clear，需交付的数据采用有限复用池。与第2、3项一起设计，避免性能优化重新突破内存上界。未声称具体加速倍数。

### 15. 高带宽收益机会：文件校验导致至少约 2N 网络数据

位置：crates/pmac-transport-russh/src/files.rs:90–91、191–194、256–274。

remote_md5 是经 SFTP 完整读取远端文件来计算。上传先发送 N 字节再读回 N；下载先读 N 再重新读取远端 N，并另读本地 staging 做校验。大文件/低带宽环境下成本明显，但这是端到端校验的取舍，不是直接删除校验的理由。

建议支持受信任的远端哈希命令或服务端 hash 扩展，并保留现有方式为 fallback；基准同时评估服务器 CPU、网络和本地磁盘。

## 验证、排除项与优先顺序

- 当前工作区 cargo test --workspace --offline --locked --quiet：243 项通过（12+43+62+54+72），文档测试通过。
- Python unittest：34 项，33 项通过，1 项跳过。
- 独立协议探针：target/audit-20261115/src/main.rs；运行 cargo run --manifest-path target/audit-20261115/Cargo.toml --offline --release。
- C ABI 旧错误探针：target/audit-20261115/stale_error.py。
- Vec 分配模式实验：target/gather-live/vec_capacity.rs。
- 一度怀疑 recovery_pending 的 && 临时锁守卫造成递归加锁；子进程实际调用关闭后的 recovery_ids 返回0，未阻塞，因此撤销该推断，不列为缺陷。
- 旧报告中的 Python wait/group 整数截断已在当前代码中校验；关闭重试成功后遗留 uncertain 的旧路径也已有修正，不重复列为当前问题。第5项是不同的 create_member revision 失效路径。
- 优先修复第1–7项；第9项增加故障注入验证；性能先处理第11、12项，并按使用负载选择第13–15项。
- 未重建图索引；图结果用于定位，当前源码与探针为结论依据。不承诺已经遍历全部源码或证明不存在其他缺陷。
## C++ / C# 使用专项补充

### C#：自动回收泄漏已复现；显式 Dispose 路径不能与 finalizer 混为一谈

第1项已经用 C# 当前源码加真实 DLL 复现，不再只是 Python 类比。使用 using/显式 Dispose 并处理关闭失败可规避该特定路径；如果对象交给 GC 自动回收，不能保证原生句柄最终释放。不能因此推断正常 Dispose 一定泄漏，也不能只靠延长等待掩盖所有权丢失。

C# 的 MotorPosition 等业务方法通过 Runtime.Check 读取 last_error，因此也受第7项旧诊断误用影响；连接池 Prepare/uncertain 和文件发布误报属于原生库共享问题，C# 与 C++ 调用均受影响。

### C++17：移动赋值在 reset 失败时丢失旧 ID 的直接访问入口（待验证恢复风险）

位置：bindings/cpp/pmac.hpp:348–349、493–499。

Client 的移动赋值先 reset()，然后不检查状态便覆盖旧 id/runtime。reset 在关闭超时或 release 失败时原本会保留旧 ID 供调用者重试；移动赋值将这一恢复入口丢弃。触发条件是旧 Client 有慢清理或未确认状态，而不是每次移动都会失败。

库还有 pendingRecovery 和原生 Runtime cleanup 监督机制，本轮未完成全部故障注入，因此只能列为恢复可观测性/移动语义风险，不能断言永久泄漏。建议移动操作在无法完成旧对象清理时将其交由具备 ID 所有权的清理任务，或提供可失败的显式替换接口。普通 C++ 正常析构、显式 stop 与这个风险不同。

### C++17 Gather/GetErrors：已核对但没有发现正常用法下的新 UAF

- start 失败前原生端 stop/join，随后 C++ Context 销毁；构造异常路径具有屏障。
- 移动 unique_ptr<Context> 不改变后台回调持有的堆对象地址。
- 正常 release 等待回调结束后才销毁 Context。
- 回调异常由 stop 在 join 后读取并重新抛出，具备同步关系。
- 回调内销毁或阻塞停止属于文档明确禁止的用法，不算支持场景的新缺陷。

C++98 和 C# 目前没有 Gather/GetErrors 的对应便捷封装；C++17 和 C ABI 有。接口覆盖缺口不是已经实现的封装有 bug。第2、3项内存预算问题发生在原生层，任何通过 C ABI 接入这些流接口的语言都会受影响。
### 16. P2：C# Runtime 自身 finalizer 也可能遗留原生 Runtime（离线复现）

位置：bindings/dotnet/Pmac/Runtime.cs:25–37。

finalizer 只调用 shutdown(value,0)，仅在当次返回0时才 release。若 shutdown 返回 pending，没有重试，也没有交给原生 pmac_runtime_cleanup 监督入口，托管对象消失后丢失原生 ID。

离线实测：弃置空 Runtime → GC，runtime_alive=False；等待300ms后手工 pmac_runtime_release(id) 仍为 -14；手工 pmac_runtime_shutdown(id,1000)=0 后 release=0。原生 shutdown 的最终 join/完成标志需要后续完成确认，不是等待一会儿就必然自动变为可释放。

建议 finalizer 使用已有的原生异步 cleanup 监督机制，避免在 GC 线程阻塞等待，也避免丢失清理所有权。C++ RuntimeOwner 已调用这个监督入口（pmac.hpp:83），因此不要把这条 C# 特有问题套用到 C++。

C#复现命令：在项目根目录把 target/release 加入 PATH 后运行 dotnet run --project target/review-csharp-gc/Review.csproj -c Release。该项目直接编译 bindings/dotnet/Pmac/*.cs，未修改生产源码。