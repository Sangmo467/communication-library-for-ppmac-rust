# Rust 1.100.0 代码复审（2026-11-14）

审查当前工作区，包括此前未提交的 GetResponse passthrough、Gather/geterrors、16通道及错误透传变更。既有修改均保留。本轮对照 [Rust 1.100.0 发布说明](https://doc.rust-lang.org/stable/releases.html#version-11000-2026-11-12)，采用 GitNexus 图分析、分区源码复核、确定性回归及编译检查。

## 已确认并修复

### 1. 请求提取错误丢失正文与执行确定性

位置：C++17 Request::throwStatus、C# Request.Error、Python Request._error_locked、C++98 Error。

触发：命令已成功完成，之后对响应使用非法正则或请求不存在的捕获组。提取 API 将错误写入线程本地 last_error；原绑定却读取成功 Request 的空错误，导致正文丢失。C++98 还存在远端错误正文仅显示操作失败的问题。若直接沿用提取错误的 NOT_SENT，会把此前已执行的命令误认为未发送。

修复：立即复制匹配失败状态的 TLS 诊断，必要时回退到 Request 的持久错误，最终保证消息非空；保留命令 Request 的执行确定性。既有异常类型与状态码保持一致。MATLAB 复用 C++17 修复。

验证：离线 C ABI fixture 覆盖非法正则、error #31 持久诊断、无正文 fallback、TLS certainty=0 而 Request certainty=2。C++17/C++98 严格编译与运行、C# RequestErrors、Python回归通过。

### 2. 自定义 Session.close panic 导致永久 Closing

位置：pmac-core/src/client.rs 的 Client::close 后台任务。

触发：公开 Rust Transport/Session 实现的 close future 首次 panic。此前后台任务退出，但 close_started 保持 true，Client 无法重试关闭，Runtime shutdown 也无法完成。未证明内置 russh 会主动触发该 panic，但公开扩展接口可确定性构造。

修复：每次 Session.close 的构造与轮询均在 catch_unwind 边界内；panic 作为未确认释放处理并重试，不宣称远端已经关闭。

验证：注入一次性 panic，再阻塞下一次 close，确认仍保持 Closing/未确认占用；放行后 Client 和 Runtime 可正常关闭。

### 3. 取消关闭等待后，迟到 SSH Close 留下孤立缓存

位置：pmac-transport-russh/src/connection.rs 的 CloseState、CloseEventRegistration 与 channel_close 回调。

触发：关闭等待被取消，远端稍后确认。旧实现已删除 waiting 项，回调转而将该 ID 加入 closed；原消费者已释放，不再读取。对持续使用的 SSH 连接重复出现时可累积。

修复：取消仅把仍存在的等待项置为空，迟到确认消费该项而不再缓存；保留正常早到确认，并保证确认后的析构不会重新登记。

验证：旧行为下取消→迟到确认回归失败；修复后取消迟到、正常早到、确认后析构三个状态测试通过。如果远端始终不确认，仍保留未确认记录至连接释放，不声称这类远端故障下的记录全局有界。

## 优化与新版检查

- Gather Decoder::push 每行三次 to_ascii_lowercase 分配合并为一次；混合大小写诊断、逐字节分片回归通过。没有吞吐基准，不声称具体速度提升。
- CI 新增 cargo doc --workspace --no-deps --locked，并通过 RUSTDOCFLAGS 启用 -D warnings 与 Rust 1.100 新增的 rustdoc::invalid_markdown_table 检查。
- 复查 never type/Infallible、if let 捕获、dyn 兼容性、repr/drop 诊断、Cargo中间产物布局；未发现必须适配的源码回归。保留稳定 C ABI；未引入 Rust ABI 跨库接口。
- 不机械采用自定义分配器、Arc/Box map 或 DropGuard；现有资源守卫承载协议确认与恢复语义，替换需要独立证明与性能数据。

## 验证范围

Windows x64 / Rust 1.100.0，AWS_LC_SYS_PREBUILT_NASM=1；使用本地模拟SSH及模拟C ABI，无控制器连机。Linux/ARM未在本机运行。

- cargo fmt --all --check：通过。
- cargo clippy --workspace --all-targets --locked -- -D warnings：通过。
- cargo test --workspace --locked：236通过，0失败；文档测试通过。
- Rustdoc严格警告与新表格lint：通过。
- Python全目录27项，1项跳过，其余通过。
- C++17/C++98诊断回归编译运行、C#诊断回归：通过。
- MATLAB R2023a + MSVC2022 MEX编译、加载、关闭：通过。

日志位于被忽略的 target/review-1100-* 文件。新增绑定回归源码在 bindings/tests、bindings/dotnet/tests/request-errors 和 bindings/python/tests/test_request_errors.py。

## 图分析边界

错误处理共享入口 impact 为 CRITICAL；Gather push 为 LOW；关闭动态分派部分返回 UNKNOWN，已补查源码调用关系并以回归验证。全工作区 detect_changes 风险包含此前已有变更，不能解释为仅本轮补丁风险。GitNexus执行流存在采样上限及跨语言解析缺口，缺失边不代表没有调用者。本报告是定向审查结果，不是全仓库无bug证明。