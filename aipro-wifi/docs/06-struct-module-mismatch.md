# 总根因：华为 struct module 布局错位 —— 主线自编模块 init 从未执行（2026-08-15）

> 三轮"修复"会话（08-12/08-14/08-15）所有崩溃的**共同上游根因**。
> 一句话：OrangePi AIpro 的华为定制 5.10.0+ 内核 `struct module` 比主线多 0x20 字节，
> 用主线源码编出的模块把 `init` 指针写进 0x150 槽位，而内核从 0x170 读 → 读到 NULL
> → **内核静默跳过模块 init，insmod 却返回成功**。

## 一、证据链（全部实测）

1. 补丁版 cfg80211 insmod 返回 0（"成功"），但：
   - `iw reg show` → "nl80211 not found"（genl family 未注册）
   - `/sys/bus/platform/devices/` 无 regulatory 设备
   - dmesg 零输出（正常应有 regulatory 初始化痕迹）
   - `cfg80211_regdomain` 保持 NULL
2. 反汇编 `init_module`（.init.text）完全正常：nl80211_init → regulatory_init → alloc_workqueue 一应俱全
3. **决定性对比**（`readelf -rW xxx.ko` 的 `.rela.gnu.linkonce.this_module`）：
   ```
   cfg80211.ko (主线headers编): init_module → __this_module + 0x150
   88x2bu.ko    (华为headers编): init_module → __this_module + 0x170  ← init 正常执行过
   cleanup_module 两者都是 +0x370（差异只在 init 之前的成员，华为多 0x20 字节）
   ```
4. 修复：把 .ko 里 init 重定位的 r_offset 0x150 改 0x170（2 字节二进制补丁）→ init 正常执行

## 二、这个根因解释了此前所有的崩溃

| 事故（旧结论） | 真实机制 |
|---|---|
| 8/12 8821cu insmod → hard lockup | 8821cu + 未初始化的 cfg80211（regdomain/锁/链表全 NULL/未锁）→ 死锁 |
| 8/12-8/15 88x2bu NULL oops @ rtw_halmac_init_adapter | chip_id 错读 0x09（另一独立 bug，E2 已修） |
| E2/E3 wiphy_update_regulatory NULL oops | 驱动 probe 调进 init 从未跑过的 cfg80211 → regdomain NULL → `get_cfg80211_regdom()->dfs_region` 跳 0 |
| docs/04 aic8800 "hiusbc 平台缺陷" 的部分判断 | 需复核：aic 崩溃也可能掺入 cfg80211 未初始化因素（但 aic8800 不依赖我们编的 cfg80211？——待核实其加载序列） |

## 三、修正后的认知

1. **"hiusbc 无法驱动 USB WiFi" 不成立**（至少不再有证据支持）：
   E2 实测 88x2bu 在 hiusbc2 直连口完成 11 秒固件 bulk 下载，USB 路径健康
2. **"出厂漏装 cfg80211/mac80211" 需重新表述**：出厂 kernel 树确实没有这两个 .ko
   （与 20240924 官方镜像 diff 一致：镜像也无），但当年 wlan0 可用
   （root bash history 有 `ping -I wlan0`，NM 有 2024 年 wlan0 租约）
   → 原厂曾用驱动栈的来源待查（可能 2024 年初代镜像内置，后续镜像精简掉了）
3. 主线源码在本机可用，但**必须对齐华为 ABI**：
   - 无线边界：cfg80211.h（wireless_dev 少 mgmt_registrations_lock）、
     mac80211.h（注释级差异）、nl80211.h uapi（完全一致）
   - 模块装载：struct module（init 槽位 0x170 vs 主线 0x150）
   - 整树 overlay include/ 会撞 uaccess/sched API 代差（华为深度回移植），不可行

## 四、当前工作配方（E4-C，已验证到 insmod init 执行）

```bash
# 1. 主线 5.10 树 + 仅换华为无线 ABI 头
#    /usr/src/linux-headers-5.10.0+/include/net/{cfg80211,mac80211}.h
#    → 覆盖 /tmp/linux-5.10/include/net/ 同名文件
#    （ieee80211.h、uapi/nl80211.h 本来就一致）
# 2. 源码 3 处适配：
#    mlme.c:    mgmt_registrations_lock → event_lock (12处，spinlock 语义保持)
#    core.c:    删 spin_lock_init(&wdev->mgmt_registrations_lock) (1处)
#    util.c/rx.c: ieee80211_data_to_8023_exthdr 补第 6 参 bool is_amsdu
# 3. 构建：zcat /proc/config.gz > .config; echo "+" > .scmversion;
#    make ARCH=arm64 olddefconfig modules_prepare
#    make ARCH=arm64 M=net/wireless modules; M=net/mac80211 modules
# 4. 二进制补丁 init 重定位（/tmp/patchko.py，scp 到机器后 sudo python3 执行）
# 5. 加载：panic_on_oops=0 → insmod cfg80211 → insmod mac80211 → insmod 88x2bu
```

88x2bu 侧（/mnt/disk/lwboy/projects/drivers/88x2bu，git 管理）：
- E1: c820 加进 RTL8822B ID 表（上游把它错放 8821C 块）
- E2: get_chip_info printk + 强制 chip_id=8822B（实测读回 0x09=8821C 值，错读）+ api 槽位 NULL 防护

## 五、遗留问题

1. chip_id 为何读成 0x09（0x0A-1）：USB 寄存器读路径 off-by-one？未深究（强制纠正已绕过）
2. `struct module` 差异的 0x20 字节具体是什么（kbox/bbox 钩子？签名域？）——可从
   /usr/src/linux-headers-5.10.0+/include/linux/module.h diff 主线得知，待记录
3. E4-C 之后：net_device 等更深结构是否也有错位（88x2bu 华为头 vs cfg80211 主线头）——
   若 wlan0 出现后行为异常，优先排查 include/linux/netdevice.h 差异
4. aic8800（绿联 AX900）崩溃是否纯平台问题，值得用修复后的 cfg80211 复测一轮
