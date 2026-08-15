# 88x2bu 驱动（RTL8812BU / RTL8822B）

> ⚠️ **当前状态：此驱动在 aipro（OrangePi AIpro 5.10.0+ 华为定制内核）上 probe 触发 NULL pointer oops，wlan 不通。**
> 详见 [../../docs/03-88x2bu-null-oops.md](../../docs/03-88x2bu-null-oops.md)。
> 8821cu（出厂）则 hard lockup（见 [../../docs/02-8821cu-probe-lockup.md](../../docs/02-8821cu-probe-lockup.md)）。
>
> **结论：8812BU 这块卡在 aipro 上不可用，建议换 ATH9K 网卡**（主线 in-tree，见 [../recommended-hardware.md](../recommended-hardware.md)）。

## 源码

上游：[morrownr/88x2bu-20210702](https://github.com/morrownr/88x2bu-20210702)（v5.13.1，2021，之后 archive，无更新版）

```bash
git clone https://github.com/morrownr/88x2bu-20210702.git
cd 88x2bu-20210702
```

## 必需的 patch（apply 顺序）

| patch | 作用 | 为什么 |
|---|---|---|
| `patches/0001-add-c820-to-rtl8822b-block.patch` | 把 `0xC820` 从 `#ifdef CONFIG_RTL8821C` 块复制一份到 `CONFIG_RTL8822B` 块，`driver_info=RTL8822B` | 上游把 c820 放错 #ifdef 块，88x2bu 编译（CONFIG_RTL8822B）会排除 c820，导致驱动不认这块卡 |
| `patches/utsrelease-hack.md` | 编译前把 headers 的 `UTS_RELEASE` 改成 `"5.10.0+"` | out-of-tree 模块 vermagic 来自 `utsrelease.h`，不改则 vermagic=`5.10.0`（无+），加载被拒 |

```bash
cd 88x2bu-20210702
git apply /path/to/aipro-wifi/driver/88x2bu/patches/0001-add-c820-to-rtl8822b-block.patch
```

## 编译（在 aipro 上）

```bash
# 1. 修 vermagic（必需，否则 vermagic 不匹配加载被拒）
sudo sed -i 's|"5.10.0"|"5.10.0+"|' /usr/src/linux-headers-5.10.0+/include/generated/utsrelease.h

# 2. 编译（KSRC 必须指 headers，/lib/modules/.../build 是坏链接）
make ARCH=arm64 KSRC=/usr/src/linux-headers-5.10.0+ -j"$(nproc)"
# 产物：88x2bu.ko

# 3. 校验
modinfo 88x2bu.ko | grep vermagic   # 必须 5.10.0+ SMP mod_unload aarch64
modinfo 88x2bu.ko | grep v0BDApC820 # 必须出现（c820 已编入）
```

## 加载（⚠️ 会 NULL oops，wlan 不通）

```bash
sudo sysctl -w kernel.panic_on_oops=0   # 必需！否则 oops 触发重启
sudo insmod /lib/modules/5.10.0+/kernel/net/wireless/cfg80211.ko
sudo insmod /lib/modules/5.10.0+/kernel/net/mac80211/mac80211.ko
sudo insmod 88x2bu.ko rtw_drv_log_level=3
# → probe c820 → rtw_halmac_init_adapter NULL pointer oops（dmesg）
# panic_on_oops=0 让系统继续，ssh 不掉，但 wlan0 不出现
```

## 已验证的对比

| 驱动 | probe 结果 | wlan0 |
|---|---|---|
| 8821cu（出厂，RTL8821C hal） | hard lockup → watchdog 重启 → ssh 断 25min+ | ❌ |
| 88x2bu v5.13.1（正确 RTL8822B hal，含 c820 patch） | NULL oops in `rtw_halmac_init_adapter`，可恢复 | ❌ |
