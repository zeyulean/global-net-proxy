# 8821cu probe hard lockup 事故记录

**日期**：2026-08-12
**影响**：aipro 整机 hard lockup → 硬件 watchdog 重启 → SSH 不可达 25 分钟+（两次）
**根因**：**驱动型号 mismatch** —— RTL8812BU 网卡被 `8821cu` 驱动当作 RTL8821C 初始化，probe 时硬件操作不匹配 → CPU 死锁

## 硬件与驱动

- USB 网卡：`lsusb` → `0bda:c820 Realtek 802.11ac NIC` = **RTL8812BU**
- 出厂驱动：`/lib/modules/5.10.0+/8821cu.ko`（DKMS `rtl8821cu/5.12.0.4`）
- 内核：5.10.0+（OrangePi AIpro，昇腾 NPU），`CONFIG_CFG80211=m`（出厂漏装 .ko，已补编见 [01-cfg80211-rebuild.md](01-cfg80211-rebuild.md)）

## 根因（型号 mismatch）

`8821cu` 驱动的 `os_dep/linux/usb_intf.c` 把 c820 映射到 RTL8821C：

```c
{USB_DEVICE_AND_INTERFACE_INFO(USB_VENDER_ID_REALTEK, 0xC820, 0xff,0xff,0xff),
 .driver_info = RTL8821C}, /* 8821CU */    ← 注释错误，c820 实为 8812BU
```

- c820 = RTL8812BU，正确驱动应是 **`88x2bu`**
- 但 8821cu 驱动（出厂版 + 社区 morrownr/8821cu-20210916 都一样）把它当 RTL8821C
- 社区版 README 明确声明只支持 8811CU/8821CU/8821CUH/8731AU，**不含 8812BU**
- probe 时用 RTL8821C 的 hal 初始化 RTL8812BU 硬件 → 寄存器/固件操作错位 → hard lockup

## 事故过程

1. **第一次**：补编好 cfg80211/mac80211 后，直接 `insmod 8821cu.ko`（设备在位）→ 立即 hard lockup → watchdog 重启 → ssh 断 25 分钟
2. **第二次（隔离诊断）**：
   - `sysctl kernel.panic_on_oops=0`
   - `echo 1-1 > /sys/bus/usb/drivers/usb/unbind`（摘设备）
   - `insmod 8821cu.ko` → **注册成功**（`usbcore: registered new interface driver rtl8821cu`），cfg80211 与 8821cu ABI 兼容，无 oops
   - `echo 1-1 > .../usb/bind`（重新 probe）→ 再次 hard lockup → 重启

| 阶段 | 结果 |
|---|---|
| 驱动注册（device unbind） | ✅ 成功，ABI 兼容 |
| probe 真实硬件 | ❌ hard lockup，watchdog 重启 |
| `panic_on_oops=0` 救得住？ | ❌ 是 CPU 死锁（关中断死循环），不走 oops 路径 |

## panic 现场未落盘

- journald volatile（`/run/log/journal`）→ 重启即丢
- rsyslog `kern.log` 持久，但 hard lockup→watchdog 即时重启，异步来不及落盘
- pstore 未配
- 若要复现捕获：先配 ramoops/netconsole，且接受再次物理重启

## 防护

- **不再 `insmod 8821cu`**（任何版本，未经隔离 + 正确型号验证前）
- cfg80211.ko/mac80211.ko 保留 `/lib/modules/5.10.0+/kernel/net/`（安全，未 autoload）
- 系统当前未 autoload 8821cu（`/etc/modules`、`modules-load.d` 均无）→ 重启安全，不会循环

## 后续可行方向（均需受控 probe，非零风险）

1. **找正确的 `88x2bu` 源码编译**（morrownr 仓库已 404/archive，需找 gitee 镜像或别的 maintainer）→ 受控 probe
2. **patch 8821cu 的 usb_intf.c**：把 c820 的 `driver_info` 改成正确 RTL8812B（驱动源码含 `rtl8814bu_set_hal_ops`，证明有 8814/8812BU hal，但需确认常量名）→ 受控 probe
3. **换主线驱动网卡**（ATH9K AR9271 / MT7601 等内核 in-tree）→ 仍需 probe
4. **换硬件**（最稳，零 probe 风险）
5. **外接路由器**：aipro 不做 AP，只做透明代理网关

社区参考：
- [morrownr/USB-WiFi #414](https://github.com/morrownr/USB-WiFi/issues/414)（Realtek ARM64 lockup）
- [morrownr/8821cu-20210916](https://github.com/morrownr/8821cu-20210916)（社区版，仍 mismatch）
- [Red Hat hard LOCKUP](https://access.redhat.com/solutions/7036727)
