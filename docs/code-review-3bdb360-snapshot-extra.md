# 3bdb360 固定快照补充审核（含未定义行为）

日期：2026-11-12。基准提交：`3bdb36087c008f075ed619e8ce6ed7e2cb7f1b00`，根树：`90e6126863e2544b584e3c071d621dd5d142092c`。

用户正在修复前轮问题。本报告只检查上述不可变提交，不判定工作区修复进度，不读取或覆盖正在修改的生产源码。源码通过直接读取 Git 对象并在内存中固定，未使用 Git CLI 提取或分析源码；GitNexus MCP 仅辅助符号与调用关系检查。

本轮在前一报告 3 项之外，补充 2 项 P2：一项有明确触发条件的 C++ 数据竞争 UB，一项 Runtime 析构遗留句柄问题。两项均为静态源码结论，未执行编译、复现或动态分析。

## N4 / P2：C++98 Options::nativePtr 并发调用会产生数据竞争 UB

位置：[pmac_cpp98.hpp:38](../bindings/cpp/pmac_cpp98.hpp#L38)、[pmac_cpp98.hpp:40](../bindings/cpp/pmac_cpp98.hpp#L40)。

快照中的实现：

```cpp
pmac_request_options* nativePtr() const {
    native_cache = native();
    return &native_cache;
}
mutable pmac_request_options native_cache;
```

触发条件：调用方让两个线程同时对同一个 Options 对象调用 nativePtr，或者一个线程通过返回指针读取，另一个线程再次调用 nativePtr。即使配置字段没有变化、两次写入相同值，也会在同一缓存上发生无同步的写写或读写访问；在采用 C++11 及以后内存模型的多线程构建中，这是数据竞争，属于未定义行为。

影响边界：这是公开 helper 的条件性问题。快照中 Client 的正常包装方法调用 native() 并使用局部副本，没有通过此 helper；不能据此声称所有 C++98 包装调用都存在 UB。GitNexus 没有解析到内部 nativePtr 调用者，与该边界一致，但不代表外部用户无法调用它。

修复建议：优先删除或停止公开这个未使用的缓存 helper，调用方使用 native() 返回的值和局部变量。若必须保留返回指针的 API，仅在方法内部加锁不能保护返回后的读取；需要明确的所有权和同步生命周期设计。

建议验证：用两个线程共享一个 Options 对象，在支持线程模型的测试程序中调用 nativePtr，并以 ThreadSanitizer 检查。修复后同样覆盖共享只读配置、每次生成局部 native 副本的路径。本轮未运行该验证。

## N5 / P2：C++ Runtime 析构在 shutdown 超时或 release 失败后丢失原生 ID

位置：[pmac.hpp:79](../bindings/cpp/pmac.hpp#L79)、[pmac_cpp98.hpp:68](../bindings/cpp/pmac_cpp98.hpp#L68)。

两个析构都只调用一次 pmac_runtime_shutdown(id, 1000)。若返回 SHUTDOWN_PENDING，直接结束析构；若 shutdown 成功，则调用 pmac_runtime_release，但忽略其失败结果。对象销毁后没有 SDK 清理所有者继续使用该 ID。

触发场景：网络协议任务、连接关闭或阻塞 I/O 的退出耗时超过 1 秒，最后一个 C++ Runtime 所有者析构。原生层按契约保留 ID，稍后后台线程可能退出，但 C++ 所有者已经消失。C++98 中若原生子句柄尚未释放，release 返回 SHUTDOWN_PENDING 后也有相同的 ID 遗留问题。

证据：原生 Runtime 后台线程只设置 runtime_stopped；joined 和 join handle 的回收由后续 shutdown 调用完成。pmac_runtime_release 还要求 shutdown 完成且没有子句柄，成功后才从 Registry 移除 ID。见 [runtime.rs:172](../crates/pmac-core/src/runtime.rs#L172)、[runtime.rs:196](../crates/pmac-core/src/runtime.rs#L196)、[lib.rs:63](../crates/pmac-ffi/src/lib.rs#L63)。

影响：遗留原生 Runtime 注册表条目和相应资源。注册表的 Runtime 上限是 16；反复触发此场景可耗尽进程的 Runtime 名额。这是资源生命周期问题，本报告不将其归类为 UB。

修复建议：显式关闭返回 Pending 时保留可重试的所有者；析构应将未完成清理交给受监督的清理所有者，或等待确认后完成 release。不能忽略 release 失败。C++98 同样需要解决 Runtime 与其子句柄的所有权关系。

建议验证：受控 shutdown 第一次返回 Pending、后续返回 OK，验证最终恰好释放一次 ID；另测 shutdown 成功但子句柄导致 release 暂时失败的情况。增加真实协议退出延迟大于 1 秒的测试，重复次数超过 16 后仍应可创建 Runtime。本轮未运行这些验证。

## Rust FFI 的 UB 检查结果与边界

已在固定快照中盘点自有 crates 的 unsafe 使用，并检查 FFI 的 types、helpers、lib、business、pool 和 registry。自有核心、协议、传输 crate 使用 forbid(unsafe_code)；实际生产裸指针操作集中在 FFI。第三方依赖及 vendor 内部 unsafe 未逐行完成审计。

本轮未确认“调用方遵守 C ABI 缓冲区与生命周期契约仍会触发”的 Rust FFI UB。核对点包括：

- 输入结构先检查尺寸和 ABI，使用 read_unaligned 复制；公开 ABI 字段是整数、浮点和裸指针，不直接反序列化 Rust bool 或枚举判别值。
- 字节输入检查非空长度下的 null，以及 isize::MAX 上界；文本转为库拥有的 String 后才异步提交。
- 输出来自由 Arc 保活的请求结果；注册表释放 ID 不会直接使已经取得的对象引用悬空。
- 池成员数组输出检查容量乘以元素尺寸的 isize 上界。
- C++17 提交 ID 槽位和同步请求选项缓存使用 thread_local，不能将其误报为跨线程共享普通字段。

FFI 不能可靠验证任意非空地址是否有效、调用方提供的实际分配长度是否与声明一致，或调用方是否在原生读取期间并发释放内存。非法、已释放、未对齐的输出指针和跨线程共写同一输出缓冲等违反 ABI 前提的使用，不在没有额外证据时作为库自身的新缺陷。catch_unwind 也不构成 UB 的防护。

尚未运行 Miri、AddressSanitizer、ThreadSanitizer 或故意构造越界指针的测试。不能把这次静态检查解释为整个库及依赖已被证明不存在 UB。

## 快照追溯与工作区隔离

- C++17 头文件 blob：`6dcc5f6f59dd1c8485de7c717754b6872978ddb9`。
- C++98 头文件 blob：`71891f97817f0a975a88ee14759651395466a9af`。
- MCP 图索引仍为 f926556，落后固定快照 4 个提交；图只用于结构辅助，代码证据来自上述提交的 Git 对象。
- 前轮 N1–N3 见 code-review-3bdb360.md。本报告不重复计数，也不判定用户正在进行的修复是否已经解决它们。
- 所有源码行号均属于 3bdb360；本地链接打开时可能显示用户随后修改的工作文件，应以报告注明的提交为准。
- 本轮只新建本报告，未修改、提交或覆盖生产源码；本报告尚未提交。
