# GitNexus 审核与修复（2026-11-13）

范围：内部 master 当前工作区（基准 4cd443c，包含原有未提交修改）。本轮聚焦新增程序缓冲区流程、Python 池管理边界和已有审查问题的现状复核；未改动公开工作区，也未提交或推送。

## 本轮确认并修复

1. 程序打开识别不接受协议允许的额外空白。例如 `open  prog 1` / `open\tprog 1` 能发送成功，却不建立线程程序上下文，后续 `dwell 10` 被当成在线命令而拒绝。改为按空白分词识别 open 与缓冲区种类，支持坐标系前缀及紧贴编号。
2. 复合命令将 open 放在后续分段时绕过单独确认检查。例如 `P1;open prog 1` 曾可提交，却不能正确跟踪远端进入的程序模式。现在检查每个在线分段并拒绝复合 open；遇到 cpx/cx 则停止扫描，保留其正文透传语义。解析入口与直接 submit 共享该检查。
3. `CLOSE  all\tbuffers` 虽按维护命令执行，却未失效其他线程的程序上下文。现在以空白分词和大小写归一匹配全局关闭，复用既有上下文失效与失败退役逻辑。
4. Python PoolProfile 和恢复版本号没有在 ctypes 转换前完整校验。超大值会截断到有效值；恢复版本号截断还可能错误匹配当前 revision。现在校验整数类型、原生范围、池大小顺序和非零连接预算，在调用原生函数前拒绝非法值。

另修复 connection.rs 和相关 EOF 测试的 rustfmt 格式差异；所有文本写入使用 PowerShell，编码 UTF-8 无 BOM。

## GitNexus 证据与边界

已重新运行 analyze --index-only；首次因用户目录注册表写入权限失败，随后以授权权限完整重建成功，最终代码修改后再更新索引。

opens_program_buffer 的 upstream impact 为 HIGH，直接调用者为 parse_command_for_current_thread、submit、execute_data，涉及提交及执行流程；本轮在修改前已报告风险。_pool_profile 的两个直接调用者为 pool_prepare_async 和 pool_resize_async。pool_confirm_recovery 和部分 Rust 方法的图结果为 UNKNOWN，已用源码调用点和回归测试补充确认，没有将空调用集作为安全证据。

索引器提示跨语言字段关联及执行流程枚举存在覆盖限制；图用于定位与影响分析，不能据此证明所有路径无缺陷。既有复审记录中的多个问题在本轮开始前已修复，本报告不将它们重复计为本轮修复。

## 验证

- 新增回归先复现程序空白识别与 Python 参数拒绝失败，再验证修复后通过。
- cargo test --workspace --locked --offline --quiet：192 项通过。
- Python 3.9 unittest discover：24 项，23 项通过，1 项 POSIX fork 测试在 Windows 跳过。
- cargo fmt --all --check：通过。
- cargo clippy --workspace --all-targets --locked --offline -- -D warnings：通过。
- scripts/Test-DependencyPatch.ps1：通过，91 个 vendor 文件校验一致。
- git diff --check：通过。

未执行真实控制器运动、物理断网、跨平台运行或长时间压力测试。

最终 GitNexus 检查：索引更新成功（3,691 节点、10,154 边、324 个已枚举流程）；detect_changes(scope=all) 返回 198 个变更符号、105 个受影响流程、35 个已跟踪变更文件，risk_level=critical，未标记 partial/truncated。该统计覆盖原有未提交修改与本轮修复，不能理解为本轮引入 198 处修改，也不等于缺陷数。未跟踪新增文件的完整内容通过源码与构建/测试补充检查。
