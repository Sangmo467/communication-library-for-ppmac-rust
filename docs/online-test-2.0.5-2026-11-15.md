# 2.0.5 在线回归（2026-11-15）

设备192.168.0.200，使用本机从当前2.0.5源码构建的Release DLL，原生版本字符串确认2.0.5。SSH沿用本会话固定主机指纹。未记录密码，未执行电机运动命令。此次不是下载GitHub附件后的二进制验收。

- 普通GetResponse读取Sys.ServoCount成功，使用当前Python绑定正常调用，无旧探针options monkey patch。
- 三轮Gather通过同一Client连续启动、停止、释放，均未分配PTY。
- 第一轮Sys.ServoCount.a，Period=10，batchRows=37，约6秒596行；相邻计数差10，回调first_sample连续。
- 第二轮Motor[1/2].ActPos.a，Period=1，batchRows=129，采样约3秒后Enable=0，排空后观察0.7秒行数不再增长，再Enable=3继续约3秒，合计5982行；电机静止位置为0。暂停恢复成功。
- 第三轮Sys.ServoCount.a，Period=1，batchRows=128，约6秒5964行；相邻计数差1。
- 各轮均无错误回调、无first_sample断裂、无非有限数值；stop/release均0。停止后观察无后续回调，Gather.Enable=0，进程、FIFO、锁清理成功。
- geterrors以0.1秒周期、默认掩码监控3秒，收到5行设置回显，均status=0。监控同时GetResponse读取Sys.ServoCount成功；stop/release均0，停止后无后续回调。未人为制造设备错误，因此未验证真实故障转换事件。
- 最终远端无gather/nc残留进程，无会话锁及FIFO。本次以-t0.100000启动的geterrors已退出。设备另有PID2352的geterrors，参数为-m0x27fc -c0x1800000008027fc -g0x6fd -r0x0，与本次默认掩码及周期不同，保留未终止，不能声称设备完全没有geterrors进程。Client及Runtime正常退出。

脚本位于target/gather-live/online205.py与monitor205.py，通过环境变量取得密码。未进行运动、断网、队列积压、异常终止或多控制器压力测试；也未声称在线覆盖C++/C#语言封装全部路径。未修改业务实现、未提交或推送。