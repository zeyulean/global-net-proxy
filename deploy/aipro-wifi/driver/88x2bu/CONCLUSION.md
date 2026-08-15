# 结论存档 2026-08-15

本卡 c820 实为 8821CU 硅片（chip_id=0x09 为真实读数，非错读）。
E2 的强制 chip_id=8822B 补丁是误诊——8822B 固件灌 8821C 导致 dlfw failed。
本仓库整条 88x2bu 线为弯路，wlan 已由原厂 8821cu.ko + openEuler 同源 cfg80211/mac80211 修复。
E1 的 c820 ID patch 与 E3 反汇编技术仍有参考价值。
勿再向本卡加载 88x2bu。
