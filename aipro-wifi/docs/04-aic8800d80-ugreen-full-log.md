# 绿联 AX900 (AIC8800D80) 修复全记录 — 2026-08-14/15

> 会话：让绿联 AX900 USB 网卡在 aipro（OrangePi AIpro 20T，昇腾 Atlas 200I A2，
> kernel 5.10.0+ aarch64）上工作 → 做无线路由。结论：**hiusbc 平台缺陷，不可修**。

## 一、硬件与 USB 拓扑（实测确认）

```
绿联 AX900 (CM763, 芯片 AIC8800D80) USB ID 生命周期:
  a69c:5724 "Aic MSC"   存储模式(带Windows驱动的3.9MB盘) ← 插入初始态
  → udev eject → a69c:8d80 "AIC Wlan" bootrom
  → aic_load_fw 下载固件 → USB 断开重枚举
  → 368b:8d88 "AIC 8800D80" 运行模式（固件常驻，aipro 重启不掉电则保持）

aipro USB 控制器（全部海思私有 hiusbc）:
  hiusbc1 (a5080000) mode=2 device/gadget 模式 → usb0 网卡，不能 host
  hiusbc2 (a5100000) mode=1 u2only=1，仅 1 个物理口(maxchild=1)
                     → 被板载 RTL8812BU(0bda:c820) 焊死占用 → Bus001
  hiusbc3 (a5180000) mode=1 u2only=0 → Bus003，Genesys hub(05e3:0610)
                     → 所有外部 USB 口都汇到这（换口无用，均经 hub）
```

## 二、修复时间线（两套驱动 × 21 轮实验）

### 阶段 1：绿联官方 V1.4 驱动（E1-E21）

源码 git: `/mnt/disk/lwboy/projects/drivers/aic8800-ugreen/`
（每步 commit + logs/EXPERIMENTS.md 实验记录）

| 实验 | 内容 | 结果 |
|---|---|---|
| 基建 | 驱动下载/固件安装/udev 规则/内核 build 软链 | ✅ |
| 兼容补丁 | `current->cpu`→`smp_processor_id()`(5.10 无该成员×4处)；CONFIG_USB_BT=n(D80 单 interface，三通道配置必崩)；KBUILD_EXTRA_SYMBOLS 解决跨模块符号(勿用 stub，曾致 duplicate symbol) | ✅ 编译通过 |
| E6 | fdrv ID 表移除 8d80（bootrom 归 aic_load_fw，消除双模块竞争 probe） | ✅ 消除引导期崩溃 |
| 超时补丁 | aicwf_txrxif.c rx/tx 线程初始化死等改超时 | ✅ |
| E7 | PWR 系列打点（probe 路径 printk+KERN_ALERT） | 定位工具 |
| E8 | rwnx_cmd_malloc 锁内 mdelay(100) 移出 + cmd 判空 | 无效（但修了真 bug） |
| E9-E16 | A(send_msg)/C(submit_urb)/E(msg_wait)/G(wake_up)/F2(rx_complete)/I(cfm_wait) 全链路打点 | 逐步收窄 |
| E10 | 去 URB_ZERO_PACKET | 无效 |
| E17 | 跳过 rf_calib（校准已由 load_fw patch 表完成） | 崩点漂移，无效 |
| E21 | rx/tx 线程绑核 CPU1→CPU2（对齐 xHCI usb3 中断核） | 无效 |

### 阶段 2：Radxa 量产版驱动（R1-R4）

源码 git: `/mnt/disk/lwboy/projects/drivers/aic8800-radxa/`
（radxa-pkg/aic8800，Rockchip 5.10 量产验证版，经 lwtop+ghfast.top 镜像下载）

| 实验 | 内容 | 结果 |
|---|---|---|
| R1 | fdrv 加 368b:8d88 ID + chipmatch D80 分支 + 宏补齐 | 编译✅ |
| R2 | URB 数量 20/100/200 → 4/8/8（减 xHCI 压力） | 无效 |
| R4 | probe 入口 `usb_disable_lpm()`（禁 USB2 L1） | 无效 |

**同样崩** → 坐实平台问题，非驱动代码问题。

### 阶段 3：8821cu 误踩（历史重演）

08-12 已确认板载网卡是 **RTL8812BU**（0bda:c820），`8821cu.ko` 是型号错误的驱动。
本会话误将 8821cu.ko + cfg80211 加载 → **再次 hard lockup**，SSH 失联 6min+，
watchdog 未救回，需物理重启。教训：**动手前先读项目已有 docs**。

## 三、崩溃特征与根因

**特征**（21+ 轮现场归纳）：
- 崩溃点随机漂移：usb_init / rx 线程初始化 / userconfig 下发 / cfm_wait(id=69 rf_calib, id=7b) / 甚至 insmod 符号解析失败时
- **无任何 Oops/Call trace**（printk 都来不及，watchdog 级静默复位）
- 加 printk 打点会暂时改变时序（崩溃点推后）→ 典型竞态
- 偶发完全僵死（bbox 不复位，只能物理重启）

**根因**：昇腾私有 USB 主控驱动 `drv_hiusbc.ko`（/var/davinci/driver/，二进制无源码）。
`strings` 直接自证已知缺陷：
```
"StartXfer and LPM request conflict on EP%u"
"bulk not always idle"
```
hiusbc 的 bulk 传输与链路电源管理(LPM)存在竞态，WiFi 类驱动的密集 bulk URB
流量模式必然踩中。佐证：
- RTL8812BU(板载,hiusbc2) 当年同样 lockup（08-12 记录）
- 4G modem / U 盘 / hub 等非密集 bulk 设备正常
- 网卡本体完好（Mac 上完美识别）

## 四、攻过的路 & 为什么不通

1. **换驱动**（绿联V1.4→Radxa）：两套独立代码同崩
2. **减负**（URB 数量、去 ZERO_PACKET）：无效
3. **禁 LPM**（usb_disable_lpm 设备侧）：无效，probe 第一行就死
4. **绑核**（线程↔xHCI 中断同核）：无效
5. **kprobe 拦截 hiusbc LPM 函数**：函数全 inline，kallsyms 无独立符号，不可行
6. **换物理口**：所有外口都汇 hiusbc3 同一控制器+hub
7. **hiusbc1 当 host**：mode=2 是 gadget 模式，硬件不支持
8. **hiusbc2 借口**：唯一口被板载网卡焊死

## 五、遗留资产（在 aipro /mnt/disk/lwboy/projects/drivers/）

- `aic8800-ugreen/` — 绿联 V1.4 + 全部补丁 + 打点，git 完整，可复现
- `aic8800-radxa/` — Radxa 版 + 8d88 ID 支持（这套 ID 补丁对其他 5.10 平台有价值）
- `/home/lwboy/aic8800/aic_load.sh` — 标准加载序列
- `/home/lwboy/aic8800/dmesg_follow.log` — watcher 崩溃现场（aic-dmesg-watcher systemd 服务持续抓取）
- `/etc/modprobe.d/aic8800-blacklist.conf` — **开机安全保证（勿删）**
- `/lib/firmware/aic8800D80/` — 全套固件

## 六、若将来重启此方向

1. 等华为/社区更新 hiusbc 驱动或内核（关注 Ascend Ottawa 社区固件包）
2. 绿联 AX900 用于标准 Linux/Mac（免驱，实测 Mac 完美识别）
3. aipro 上 WiFi 需求 → 外接路由器发射，aipro 只做网关（README 既定路线 4）
