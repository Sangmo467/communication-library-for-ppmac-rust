# 复位在线复现及输入提示过滤（2026-11-16）

设备192.168.0.200，原生版本2.0.5，沿用本会话固定SSH主机指纹。经用户确认执行复位；未执行save或运动命令。脚本target/gather-live/reset_online.py，密码只从环境变量读取。

过滤修改前实测：
- 透传 $$$，timeout=15000ms，约8.031秒成功返回 Reset Complete + STDIN Open for ASCII Input；同Client后续Sys.ServoCount成功。
- 透传 $$$***，约15.234秒返回ExecTimeout(-5)、MayHaveExecuted；Client状态7，后续提交返回-27。新Client可读计数，表明设备已经复位；不能据超时判定未执行。
- 最后以分类模式发送一次 $$$ 加载已保存配置，约8.422秒成功返回复位提示；原Client状态7，后续同通道读取-23；新Client查询成功，Gather.Enable=0。
- 最后一次分类复位Client释放返回-14，导致脚本Runtime上下文关闭也返回-14。该现象是尚待处理的复位后旧Client清理/恢复问题，没有在此声称修复。恢复已保存配置由复位响应及新Client可读确认，但不能声称恢复了测试前未保存的活动配置。

用户随后要求任何时候过滤 STDIN Open for ASCII Input。本次在GPASCII完整帧的返回体及ResetDecoder的最终返回体删除该固定字节文本；普通、透传、分类复位共用。内部ResetDecoder仍用原始提示检测就绪，并验证只读探测，不把字符串删除当作完成依据。只过滤面向调用者的命令响应，不改变裸SSH终端或外部工具输出。

离线验证：协议65项+传输73项=138项通过，含分包边界提示过滤及原有复位就绪探测回归；完整workspace Clippy all-targets -D warnings通过。重建Release DLL，未重新发送复位命令，未提交或推送。
## 后续修复与在线复核

修复内容：默认透传的独立 $$$/$$$*** 保留复位维护屏障和专用响应流程，其他opaque文本不变；retire_device按旧session自身retire_control结果记录释放不确定性，close按同一session确认成功清除其历史uncertain。未自动confirm底层预算、未重放复位。

在线复核：target/gather-live/reset_fixed_online.py，仍为本地构建2.0.5 DLL，显式timeout=30000ms。
- 默认透传 $$$***：约8.250秒返回 Global Reset Complete，未包含STDIN Open for ASCII Input；旧通道失效后显式pool_repair，同Client可读Sys.ServoCount=2792，state=4。
- 最后默认透传 $$$：约8.422秒返回 Reset Complete；显式pool_repair后同Client可读Sys.ServoCount=2810，state=4。
- Client和Runtime正常退出，CLOSED_ALL，无此前-14清理错误。
- 全工作区Rust测试256项及文档测试通过；Clippy workspace/all-targets -D warnings通过。Release DLL已更新，未提交推送。

调用者必须显式设置足够复位预算，默认5000ms仍可能不足；不隐式改变deadline。复位后连接池修复是显式操作，单个复位请求成功不代表旧GPASCII通道可以继续使用。