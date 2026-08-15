# aipro-wifi

> 目标：让 OrangePi AIpro 20T（昇腾，192.168.1.2）用 USB WiFi 网卡做 AP，
> WiFi 出口经 gnp 透明代理访问 global-net。
>
> **✅ 2026-08-15：wlan0 已完全可用**（客户端模式，连接 XinYuan，192.168.0.50）。
> 修复总记录见 [docs/06-struct-module-mismatch.md](docs/06-struct-module-mismatch.md)。

## 状态（截至 2026-08-15）

| 阶段 | 状态 | 说明 |
|---|---|---|
| 诊断 | ✅ | 三层根因全部闭环（出厂缺模块 → 主线源码 ABI 错位 → init 被跳过）|
| 同源 cfg80211/mac80211 | ✅ | openEuler-22.03-LTS-SP1 源码编译，init 正常执行，`5.10.0+` vermagic |
| 原厂 8821cu 驱动 | ✅ | **它一直是对的**（c820 实为 8821CU 硅片，chip_id=0x09 非错读）；此前 lockup 全因坏 cfg80211 |
| wlan0 客户端 | ✅ | 连接 XinYuan，-57dBm，200Mbps tx，ping 网关 0% 丢包 |
| AP 模式 + gnp 出口 | ⏭ 下一步 | hostapd.conf 已备（/etc/hostapd/），8821cu 支持 AP |
| 开机自启 | ⏭ 下一步 | modules-load.d + depmod（防 rfkill.ko 残留陷阱）|

## 工作配方（复现）

```bash
# 1. 同源源码：gitee.com/openeuler/kernel，分支 openEuler-22.03-LTS-SP1
#    （华为文档钦定 Atlas 200I A2 内核 = openEuler 22.03 LTS SP1 rt 变种）
# 2. 编译（aipro 上）：
zcat /proc/config.gz > .config && echo "+" > .scmversion
make ARCH=arm64 olddefconfig && make ARCH=arm64 modules_prepare
make ARCH=arm64 M=net/wireless modules && make ARCH=arm64 M=net/mac80211 modules
#    产物 vermagic=5.10.0+，init 重定位原生 0x170（结构对齐的自检标志）
# 3. 安装到 /lib/modules/5.10.0+/kernel/net/{wireless,mac80211}/
# 4. 加载（bash/aiprowifi.sh start）：
sudo sysctl -w kernel.panic_on_oops=0
sudo insmod .../cfg80211.ko && sudo insmod .../mac80211.ko && sudo insmod /lib/modules/5.10.0+/8821cu.ko
```

## ⚠️ 血泪铁律（保留有效部分）

1. **绝不用主线 kernel.org 源码编本机模块**——struct module 等结构全线错位，
   init 会被**静默跳过**（insmod 显示成功），后续死法千奇百怪（lockup/oops）
2. **判断模块 init 是否真跑**：`ls /sys/bus/platform/devices/ | grep regulatory` +
   `iw reg show`，不要信 insmod 的返回码
3. 驱动崩溃先查 cfg80211/mac80211 是否健康，再怀疑驱动本身
4. `chip_id` 读数（如 REG_SYS_CFG2）是**芯片真实身份**，比 USB ID 数据库可信——
   本卡 c820 "官方=8812BU" 实为 8821CU 硅片，出厂装 8821cu 是对的
5. 详见 [docs/06](docs/06-struct-module-mismatch.md)（总根因+完整证据链）与
   [docs/05](docs/05-wifi-repair-overview.md)（方法论，部分结论已被 06 修正）

## 文件结构

```
aipro-wifi/
├── README.md
├── docs/
│   ├── 01-cfg80211-rebuild.md       # 主线补编流程（已过时，仅存档——勿再用于本机！）
│   ├── 02-8821cu-probe-lockup.md    # 8821cu lockup 事故（根因已修正：坏 cfg80211 所致）
│   ├── 03-88x2bu-null-oops.md       # 88x2bu 弯路（根因同上；chip_id 强制补丁系误诊）
│   ├── 04-aic8800d80-ugreen-full-log.md
│   ├── 05-wifi-repair-overview.md   # 方法论（结论部分见 06 修正）
│   └── 06-struct-module-mismatch.md # ✅ 总根因 + 终局方案 + 工作配方
├── bash/
│   ├── aiprowifi.sh                 # start/stop/status（已更新为 8821cu 方案）
│   ├── build-ath9k.sh               # （未用上，ATH9K 硬件路线不需要了）
│   └── ...
└── driver/88x2bu/                   # 88x2bu 弯路存档（E1/E2 补丁勿再用于本卡）
```
