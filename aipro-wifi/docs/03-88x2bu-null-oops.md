# 88x2bu probe NULL pointer oops（2026-08-12）

`8821cu`（错型号）被证 hard lockup 后，换正确的 `88x2bu`（morrownr/88x2bu-20210702, v5.13.1, RTL8822B hal, AP_MODE=y）继续。

## 编译 + patch

1. lwtop 拉源码，aipro `/tmp/88x2bu-src` 用 `KSRC=/usr/src/linux-headers-5.10.0+` 编译（`/lib/modules/5.10.0+/build` 是坏链接）
2. vermagic 修复：out-of-tree 模块 vermagic 来自 `utsrelease.h` 的 `UTS_RELEASE`，需 `sed` 把 headers 的 `UTS_RELEASE "5.10.0"` 改成 `"5.10.0+"`（改 kernel.release / .scmversion 都无效，必须改 utsrelease.h）
3. **c820 patch**：源码 `os_dep/linux/usb_intf.c` 里 `0xC820` 行被放在 `#ifdef CONFIG_RTL8821C` 块（driver_info=RTL8821C），而 88x2bu 编译 `CONFIG_RTL8822B`，导致 c820 **被编译器排除**——88x2bu 根本不认这块卡。patch：在 RTL8822B 块（b82C 行后）加 `{...0xC820..., .driver_info = RTL8822B}`。验证 `modinfo 88x2bu.ko | grep v0BDApC820` 出现即编入。

## probe 结果（NULL pointer oops）

patch c820 后重新 insmod，USB core 立即匹配 c820 触发 probe，走到 hal 初始化触发：

```
Unable to handle kernel NULL pointer dereference at 0x0
pc : 0x0
lr : rtw_halmac_init_adapter+0xfc/0x248 [88x2bu]
Call trace:
 rtl8822bu_halmac_init_adapter+0x3c/0x48
 rtl8822bu_set_hal_ops+0x20/0x184
```

某 halmac callback 函数指针为 NULL，调用时跳 0x0。

## 与 8821cu 的对比

| | 8821cu（错型号） | 88x2bu（正确） |
|---|---|---|
| probe 失败方式 | hard lockup（CPU 死锁，watchdog 重启，ssh 断 25min+） | 普通 oops（NULL 解引用，`panic_on_oops=0` 救住，ssh 不掉） |
| 走到哪 | hal 初始化前 lockup | hal 初始化内 `rtw_halmac_init_adapter` |
| 可恢复 | 需物理重启 | ssh 自愈，aipro 不重启 |

**关键收获**：`sysctl kernel.panic_on_oops=0` + 正确型号驱动，把"必崩 lockup"降级为"可恢复 oops"——这是后续反复试错的安全基础。

## 现状判断

- v5.13.1（2021）是 morrownr 最后的 88x2bu 版本（之后 archive），无更新版可换
- nyetwurk/linux-wifi-88x2bu-driver 是 2020 老 fork，不专 8822bu，弃
- 主线 rtw88（kimocoder/rtw88-usb）只 managed/monitor，**不支持 AP**，不满足"WiFi 路由"需求
- NULL 来源推测：v5.13.1 hal init 对 OrangePi 华为定制 5.10 内核的 USB/DMA 接口兼容问题（无内核源码，难修）

## 待选路径

1. 深入 objdump 反汇编定位 NULL 寄存器来源，尝试 patch（1-2h，不保证）
2. 换主线 in-tree 驱动网卡（**ATH9K AR9271** 推荐：cfg80211/mac80211/ath9k 全主线、AP 成熟、probe 干净），配合已补的 cfg80211/mac80211，插上即用
3. 搁置

## 当前 aipro 状态（安全）

- ssh 全程通畅，eth0/路由未动
- 88x2bu loaded（probe oops 后驻留，used=1，rmmod 卡；不影响稳定）
- cfg80211.ko / mac80211.ko 在 `/lib/modules/5.10.0+/kernel/net/`（备用）
- panic_on_oops=0 已设（本 session），守护 oops 不重启
