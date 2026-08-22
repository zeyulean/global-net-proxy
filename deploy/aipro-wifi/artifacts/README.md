# artifacts — 可直接部署的修复产物（2026-08-15）

> ⚡ **小故障快速恢复**：三个 .ko 拷回 aipro 对应路径 + depmod + modprobe 即可，
> 无需重新编译。完整背景见 [../docs/06-struct-module-mismatch.md](../docs/06-struct-module-mismatch.md)。

## 文件清单与 md5（部署后必须校验）

| 文件 | md5 | 来源 | 部署目标（aipro） |
|---|---|---|---|
| cfg80211.ko | b3ce310d67ba6e873e447fa357f93b3a | openEuler-22.03-LTS-SP1 同源编译 | /lib/modules/5.10.0+/kernel/net/wireless/cfg80211.ko |
| mac80211.ko | 0b77c58831d6c8403814e2bac3aef48f | 同上 | /lib/modules/5.10.0+/kernel/net/mac80211/mac80211.ko |
| 8821cu.ko | 2168a29a3c75589dd5a0a4b845b2a736 | **原厂 DKMS 5.12.0.4 备份**（未改动） | /lib/modules/5.10.0+/8821cu.ko |
| aic8800/aic8800_fdrv.ko | 2396b6f4c2bc358f19bda87102158583 | ugreen V1.4 树全补丁 + **CONFIG_USB_BT=y**（E22） | /lib/modules/5.10.0+/kernel/drivers/net/wireless/aic8800/aic8800_fdrv.ko |
| aic8800/aic_load_fw.ko | c6aa624854aaa6d35be1f5dd0d954f47 | 同树（含 8d80 bootrom 支持） | /lib/modules/5.10.0+/kernel/drivers/net/wireless/aic8800/aic_load_fw.ko |

绿联 AX900 加载序列（bootrom a69c:8d80 → udev 弹出已自动 →）：
`insmod aic_load_fw.ko`（下载固件，设备变 368b:8d88）→ `insmod aic8800_fdrv.ko` → **wlan1** 出现。
一键脚本：`bash/aic_load.sh`；源码+实验史：`driver/aic8800-ugreen-src/`（tree.tar.gz + git bundle）。
blacklist（/etc/modprobe.d/aic8800-blacklist.conf）保留——开机不自动加载，需要时手动。

## 2026-08-22 TF 重建部署补充（eMMC 损伤换 TF 后）

**关键发现：udev `eject` 是模式切换的触发器**。USB 适配器上电先枚举为 MSC 虚拟盘（a69c:5724, 3.9M），必须 `eject /dev/sda` 后才切换到 WiFi 模式（368b:8d88）。TF 新系统无此规则则永远卡 MSC。`aic.rules` 装到 `/etc/udev/rules.d/99-aic.rules` 即自动处理。

TF 部署完整序列（2026-08-22 验证通过）：
```
模块 → /lib/modules/5.10.0+/kernel/drivers/net/wireless/aic8800/
固件 → tar xzf firmware/aic8800D80-firmware.tar.gz -C /lib/firmware/
udev → cp aic8800/aic.rules /etc/udev/rules.d/99-aic.rules && udevadm control --reload
触发 → eject /dev/sda（udev 自动或手动）
加载 → cfg80211 → mac80211 → aic_load_fw(sleep 3) → aic8800_fdrv → bind 3-1.3:1.0（如未自动）
自启 → /etc/modules-load.d/aipro-wifi.conf 五模块 + 容器 restart=unless-stopped
```

**⚠️ 清桩版（a28e6ef）在 TF 系统编译可过但 insmod 报 Unknown symbol**——vendor 内核非导出符号未在 headers 的 Module.symvers 里。原 eMMC 上 08-19 编译的清桩 .ko 能用但已随 eMMC 丢失（TF 启动 eMMC 不可见）。当前 artifacts 里的 fdrv 是带 AICDBG 的版本（~1.4条/s，仅 dmesg RAM 缓冲，零磁盘影响，可接受）。修复需 vendor 完整符号表。

绿联完整恢复清单（系统重刷后）：
1. `.ko` → 上表两文件部署
2. `firmware/aic8800D80-firmware.tar.gz` → 解压到 /lib/firmware/（2.1M）
3. `aic.rules` → /etc/udev/rules.d/（插入时自动把 a69c:5724 存储态弹出为 bootrom）
4. `bash/aic_load.sh` → 执行加载

vermagic 均为 `5.10.0+ SMP mod_unload aarch64`（仅适用于 OrangePi AIpro 20T 当前内核）。

## 来源（编译配方）

```bash
# 源码：https://gitee.com/openeuler/kernel 分支 openEuler-22.03-LTS-SP1
# （华为文档钦定 Atlas 200I A2 内核血统；两个 ABI 指纹与 /usr/src/linux-headers-5.10.0+ 吻合）
git clone --depth 1 --branch openEuler-22.03-LTS-SP1 https://gitee.com/openeuler/kernel.git
zcat /proc/config.gz > .config && echo "+" > .scmversion
make ARCH=arm64 olddefconfig && make ARCH=arm64 modules_prepare
make ARCH=arm64 M=net/wireless modules && make ARCH=arm64 M=net/mac80211 modules
# 自检：产物 init 重定位必须落在 0x170（readelf -rW xxx.ko | grep -A2 rela.gnu）
```

一键脚本：[../bash/build-samesource.sh](../bash/build-samesource.sh)　恢复脚本：[../bash/restore-wifi.sh](../bash/restore-wifi.sh)

## ⚠️ 绝对禁止

- ❌ 用 kernel.org 主线 linux-5.10 源码编本机模块（struct module 等结构错位 →
  init 被静默跳过 → 各种 lockup/oops，见 docs/06）
- ❌ 向本卡（USB ID 0bda:c820，实为 8821CU 硅片）加载 88x2bu/8812bu 驱动
