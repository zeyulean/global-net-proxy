# AIC8800 驱动实验记录

## 环境
- 机器: aipro (昇腾 Atlas 200I A2, 香橙派形态), kernel 5.10.0+ aarch64
- 网卡: 绿联 AX900 (CM763-35265, AIC8800D80 芯片)
- USB 设备状态机: a69c:5724(存储/Aic MSC) -> a69c:8d80(AIC Wlan/bootrom) -> 固件加载后 368b:8d88(AIC 8800D80 运行模式)

## 实验时间线
- E1: 官方源码直接编译 -> modpost 缺符号(get_fw_path等) => 这些符号由 aic_load_fw 模块导出
- E2: 补 stub + insmod -> panic: aicwf_usb_free_urb NULL链表 (probe 失败路径链表未初始化)
- E3: INIT_LIST_HEAD 提前 + 加载 aic_load_fw 序列 -> load_fw 成功下载固件, 设备变 368b:8d88
- E4: fdrv 无 8d88 ID -> 加 8d88(a69c+368b) -> panic 在 bus_init 等待 rx/tx 线程
- E5 (2026-08-14 21:49 崩溃): dmesg_follow.log L6326: **fdrv 与 aic_load_fw 竞争 probe a69c:8d80**。fdrv ID 表含 8d80 是错误的(8d80 是 bootrom, 应归 aic_load_fw)。fdrv 用运行时协议对 bootrom 设备初始化 -> 崩溃。且 rd_version_val=00000000 说明芯片应答异常。

## 待执行 (E6)
- fdrv ID 表删除 a69c:8d80 (仅保留 368b:8d88)
- chipmatch 删除 8d80 引用
- 加载顺序: cfg80211 -> aic_load_fw(等设备8d80引导->8d88) -> fdrv(接管8d88)
