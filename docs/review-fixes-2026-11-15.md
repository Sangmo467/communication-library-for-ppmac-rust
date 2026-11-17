# 审核问题修复与优化（2026-11-15）

基于当前工作区实施，未提交或推送；版本保持2.0.4，Release DLL已重建。保留所有原有工作区改动。本记录是 code-review-2.0.4-2026-11-15.md 的实施状态补充。

## 已完成

- Python/C# Client GC清理：Runtime独立保存原生Client ID，关闭未完成时重试释放，显式Runtime关闭兜底回收。
- Python/C# Runtime finalizer使用原生cleanup监督入口，不再仅尝试一次shutdown(0)。C#新增持久finalizer回归项目。
- C++17 Client移动赋值：旧对象reset失败时抛出异常并保留双方句柄；移动赋值不再声明noexcept，移动构造仍保持noexcept。这是源码异常规格变化，应注意依赖is_nothrow_move_assignable的泛型代码。
- Gather/geterrors交付精确长度Box slice，避免几何扩容capacity随队列累积突破64MiB预算；复用行缓冲、减少纯数值诊断匹配分配。
- Gather远端终止后用pgrep确认退出；失败或超时保留锁并报告清理失败，不再直接吞掉失败解锁。增加pgrep与sleep依赖；仍遵守gather/nc专用约定。
- Prepare配额失败回滚options，同Client可重试；过期revision的连接若close确认成功不登记虚假uncertain，失败保留清理所有权。
- 请求完成时按自身ID删除请求表项，代替每次扫描全表。
- 上传提前检查生成的mv命令长度，长路径和引号膨胀在写入staging前拒绝，不再误标PublishUnknown。
- DART共享预算表在miss时清理已失效Weak条目。
- 业务C ABI参数失败统一刷新last_error；原反例现在正确返回invalid motor target诊断。
- ShellDecoder最终完成分支检查输出上限。
- GPASCII仅扫描新到达字节查找结束符，消除普通长响应的小片段重复扫描。
- C ABI正则只选择所需capture的借用切片，不再先复制所有重叠组；保留原公开regex_match完整捕获API兼容性。

## 验证

- cargo test --workspace --offline --locked --quiet：251项全部通过（13+47+64+54+73），文档测试通过。
- Python unittest：35项，34项通过，1项平台相关跳过。
- C# bindings/dotnet/tests/finalizers：当前源码加真实Release DLL运行，Runtime和Client两个GC回归通过。
- C++ bindings/tests/client_move_failure.cpp：MSVC x64编译运行通过，模拟旧Client关闭失败和正常移动赋值。
- cargo fmt --all --check通过。
- Release DLL构建通过；C ABI旧错误探针确认诊断已刷新。
- 同机Release协议微基准（128字节分片）：64KiB约6481→476微秒，128KiB约25077→855微秒，256KiB约88732→1533微秒。该数据仅是解析耗时，不是网络吞吐保证。

## 限制与保留

- 没有重新连接设备；Gather终止脚本故障注入回归仅在Unix执行，在Windows使用cfg跳过，仍需Linux CI覆盖。
- 文件校验的约2N网络读取保留，不削弱校验；用远端hash降低流量需要服务端能力协商和额外故障测试，未引入。
- 未引入全局正则编译缓存，避免未经约束的缓存增长；现有两次C ABI复制调用仍可能分别编译正则。
- 未承诺彻底消除所有内存峰值：声明的队列payload预算已修复，仍另有接收、当前批次、回调和结构体开销。