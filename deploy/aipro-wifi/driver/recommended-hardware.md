# 推荐硬件：换主线 in-tree 驱动网卡

## 为什么换

8812BU（Realtek RTL8812BU/RTL8822B）在 aipro 上是死路：

| 驱动 | 结果 |
|---|---|
| 8821cu（出厂） | 错型号 hard lockup |
| 88x2bu（正确，含 c820 patch） | hal init NULL oops |
| 主线 rtw88_8822bu | OrangePi 内核未开 CONFIG，且 AP 支持弱 |

OrangePi AIpro 的华为定制 5.10 内核**无线支持残缺**（cfg80211/mac80211 出厂漏装，ATH9K/RTL8XXXU 全没开 config），Realtek out-of-tree 驱动与其兼容性差。

**出路**：换主线 in-tree 驱动网卡，配合我已补的 cfg80211/mac80211 + 补编的 ath9k，probe 干净不崩。

## 首推：ATH9K（AR9271）

- 驱动全主线 in-tree：`cfg80211 + mac80211 + ath + ath9k + ath9k_htc`
- AP 模式成熟稳定（hostapd 支持）
- probe 干净（不 lockup/oops）
- 2.4GHz 11n（150Mbps），够 WiFi 路由用
- 便宜（二三十元）

### AR9271 常见 USB 网卡（认准芯片，别买错版本）

| 型号 | 备注 |
|---|---|
| **TP-Link TL-WN722N v1** | 经典，0cf3:9271，认准 v1（v2/v3 是 RTL8188 非 AR9271） |
| **TP-Link TL-WN821N v2/v3** | 0cf3:7062，v3 起为 AR9271 |
| **Netgear WNA1100** | 0846:9030 |
| **Alpha AWUS036NHA / AWUS036NEH** | 0cf3:9271 / 0cf3:9271 |
| **any AR9271 通用卡** | `lsusb` 看 `0cf3:9271` 即可 |

⚠️ AR9271 = USB id `0cf3:9271`。购买前问卖家芯片型号，**避开 v2/v3 的 TP-Link（RTL8188EUS，又是 Realtek out-of-tree）**。

## 备选

- **MT7601u**（Ralink）：主线 in-tree（CONFIG_MT7601U），但只 2.4GHz 11n，且 OrangePi 内核需补编 mt7601u.ko
- 其他 ath9k USB 芯片：AR7010（ath9k_htc，0cf3:7010/7015）

## 插上后的流程

1. `lsusb` 确认 `0cf3:9271`
2. 我补编的 ath9k.ko + ath9k_htc.ko + ath.ko 已就位（见 `bash/build-ath9k.sh`）
3. `insmod` 加载 → wlan0 出现（probe 不崩）
4. hostapd 起 AP，出口走 gnp 透明代理

## 不要买

- ❌ 任何 Realtek（8812/8814/8821/8822/8188）—— aipro 内核 Realtek out-of-tree 驱动都崩或不稳
- ❌ RTL8812BU/RTL8814BU/RTL8821CU/RTL8822BU 系列（就是当前这块卡的家族）
