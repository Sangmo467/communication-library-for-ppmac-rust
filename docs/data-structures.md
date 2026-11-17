# 数据结构前缀与自定义变量

[中文](data-structures.md) | [English](data-structures.en.md)

非透传模式按完整结构根名称及边界识别前缀，匹配不区分大小写，发送时保留原文。索引结构使用 Root[ 开头，单实例结构使用 Root. 开头；MotorState_Mtr 不会误匹配 Motor。

只判断结构前缀，不枚举后面的成员、嵌套结构或多维下标。比如 CompTable[0].Data[1][2][3]、BrickLV.Chan[0].TwoPhaseMode、BrickAC.Chan[0].I2tWarnOnly 都可提交。字段名称、维度、下标范围和表达式是否有效由实际控制器固件判断。仅有前缀也允许提交，但控制器可能报语法错误。

完整且可静态识别的数值下标成员查询可进入只读池；赋值、动态表达式及无法证明只读的其余正文统一作为 Control 请求发送。Control 是本地调度分类，不会改写命令为赋值。原有单行长度及协议控制字节校验仍适用。

自定义标量/数组支持 StimulusType、StimulusType(11)、MotorState_Mtr(11)；自定义数组查询不得使用 StimulusType[11]。自定义名称查询保守走主通信成员，不使用 read_pool。语法匹配不区分大小写不代表控制器的用户声明也忽略大小写，库始终保留原始名称。

## 手册核对目录

下表根据 Power_PMAC_Software_Reference_Manual 的 Saved/Non-saved/Status 结构目录核对，并补充 DART 和 Modbus 独立规范。页码为手册印刷页；Modbus: 为对应 Modbus 规范页码，DART: 为 DART 规范页码。Acc72E 来自手册历史硬件引用，Acc72EX 为当前结构目录名称，两者分别保留，不作别名替换。表格只列根名称，不表示所有固件都实现所有结构。

| 根名称 | 前缀 | 手册页 |
| --- | --- | --- |
| Acc11C | Acc11C[i] | 78,626,765 |
| Acc11E | Acc11E[i] | 78,626,765 |
| Acc14E | Acc14E[i] | 79,626,765 |
| Acc24C2 | Acc24C2[i] | 79,627,766 |
| Acc24C2A | Acc24C2A[i] | 79,627,766 |
| Acc24E2 | Acc24E2[i] | 79,627,766 |
| Acc24E2A | Acc24E2A[i] | 79,627,766 |
| Acc24E2S | Acc24E2S[i] | 79,627,766 |
| Acc24E3 | Acc24E3[i] | 79,627,766 |
| Acc28E | Acc28E[i] | 767 |
| Acc36E | Acc36E[i] | 628,770 |
| Acc51C | Acc51C[i] | 79,629,774 |
| Acc51E | Acc51E[i] | 79,629,774 |
| Acc53E | Acc53E[i] | 775 |
| Acc58E | Acc58E[i] | 80,629,777 |
| Acc59E | Acc59E[i] | 630,778 |
| Acc59E3 | Acc59E3[i] | 80,632,781 |
| Acc5E | Acc5E[i] | 78,626,765 |
| Acc5E3 | Acc5E3[i] | 78,626,765 |
| Acc5EP3 | Acc5EP3[i] | 78,626,765 |
| Acc65E | Acc65E[i] | 80,632,781 |
| Acc66E | Acc66E[i] | 80,632,781 |
| Acc67E | Acc67E[i] | 80,632,781 |
| Acc68E | Acc68E[i] | 80,632,781 |
| Acc72E | Acc72E[i] | 1690,1693 |
| Acc72EX | Acc72EX[i] | 633,782 |
| Acc84B | Acc84B[i] | 80,637,784 |
| Acc84C | Acc84C[i] | 80,637,784 |
| Acc84E | Acc84E[i] | 81,87,638,785 |
| Acc84S | Acc84S[i] | 96,639,801 |
| AdcDemux | AdcDemux. | 97 |
| BrickAC | BrickAC. | 100,103,640,803 |
| BrickLV | BrickLV. | 104,105,645,817 |
| BufIo | BufIo[i] | 107,650,822 |
| CamTable | CamTable[i] | 111,657,829 |
| Cid | Cid[i] | 660 |
| CK3C | CK3C[i] | 122,662,833 |
| CK3WAD | CK3WAD[i] | 122,662,833 |
| CK3WAX | CK3WAX[i] | 122,662,833 |
| CK3WECS | CK3WECS[i] | 122,662,833 |
| CK3WGC | CK3WGC[i] | 122,662,833 |
| CK3WMD | CK3WMD[i] | 122,662,833 |
| Clipper | Clipper[i] | 122,662,833 |
| CompTable | CompTable[i] | 123 |
| Coord | Coord[i] | 136,663,834 |
| Dart | Dart. | DART:4 |
| DPR | DPR[i] | 674 |
| ECAT | ECAT[i] | 198,680,890 |
| EIP | EIP[i] | 684 |
| EncTable | EncTable[i] | 235 |
| Gate1 | Gate1[i] | 266,279,687 |
| Gate2 | Gate2[i] | 292,312,693,697 |
| Gate3 | Gate3[i] | 325,365,704,709 |
| GateIo | GateIo[i] | 405,718,978 |
| Gather | Gather. | 724 |
| Ldata | Ldata. | 734,987 |
| Macro | Macro. | 412,994 |
| Motor | Motor[i] | 415,739,1001 |
| MuxIo | MuxIo. | 590,744,1061 |
| Plc | Plc[i] | 746,1064 |
| PowerBrick | PowerBrick[i] | 598,746,1066 |
| Sys | Sys. | 599,747,1067 |
| Tdata | Tdata[i] | 757 |
| UserAlgo | UserAlgo. | 762 |
| Modbus | Modbus[i] | Modbus:16-23 |
| ModbusServer | ModbusServer[i] | Modbus:26-28 |
