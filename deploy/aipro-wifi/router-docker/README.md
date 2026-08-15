# aipro-wifi-router — 无线路由 Docker

> **一个容器 = 完整无线路由器**：AP(hostapd) + DHCP(dnsmasq) + 透明代理(sing-box tproxy → gnp hy2/QUIC 出海)。
> 连上 SSID `aipro` 即全自动科学上网，设备零配置。

## 拓扑

```
手机/Mac ──WiFi── SSID:aipro (5G ch36, wlan1=绿联AX900)
                     │ 192.168.88.1/24
              ┌──────┴──────┐  docker: aipro-wifi-router (host网络+privileged)
              │ hostapd     │  AP (WPA2-PSK)
              │ dnsmasq     │  DHCP 192.168.88.10-200 (port=0 不做DNS)
              │ sing-box    │  tproxy:7893 → hysteria2(QUIC) → 8.209.203.17 出海
              │ iptables    │  mangle TPROXY：DNS优先劫持 + 私网直连放行
              └─────────────┘
                     │ eth0 (192.168.1.2)
                gnp hy2/QUIC ──→ global net
```

## 部署（aipro 上）

```bash
# 前提：wlan1 驱动已加载（bash/aic_load.sh；开机不自启，见 blacklist）
sudo bash /mnt/disk/lwboy/projects/aipro-wifi-router/run.sh
# run.sh 自动：取宿主 gnp hy2 密码 → NM 交接 wlan1 → docker build → run
```

### 资源依赖（sing-box 二进制 / aic8800 源码）

存于**子模块 `aipro-wifi/resources`**（同仓库分支 `aipro-resources`，
含 sing-box 1.12.3 linux-arm64 预编译 + aic8800-ugreen E22 源码）：

```bash
git submodule update --init aipro-wifi/resources
# sing-box 来源优先级：resources（离线） > aipro 宿主 gnp 部署路径
```

## 验收结果（2026-08-15，Mac 实连实测）

| 测试 | 结果 |
|---|---|
| AP 广播 | ✅ ssid=aipro, 5G ch36, AP-ENABLED |
| DHCP | ✅ 192.168.88.190 (dnsmasq) |
| DNS（隧道内 8.8.8.8，无污染） | ✅ google→142.251.x（修复前被污染成 Facebook IP）|
| google/youtube 204 | ✅ 0.30s |
| github / 2MB 下载 | ✅ 200 / 825KB/s |
| 出口 IP | ✅ 8.209.203.17（hy2 服务器）|
| 内网互通（192.168.x 直连不走代理） | ✅ ssh/管理不受影响 |

## 踩坑记录（本目录配置已含修复）

1. **hostapd 2.10**：`WPA2-PSK` 无效 key_mgmt → 用 `WPA-PSK`（wpa=2 即 RSN/WPA2）
2. **sing-box 1.12**：tproxy inbound `network` 只接受单值 → tcp/udp 各建一个 inbound（同端口可共存）；
   DNS 劫持用 route 规则 `{"action":"hijack-dns"}`（老的 experimental dns-out 已移除）
3. **DNS 劫持必须先于私网 RETURN**：AP 网关 192.168.88.1 本身在 192.168.0.0/16 内——
   DNS 若不优先 TPROXY，会被 dnsmasq(53) 应答→走宿主 resolv.conf→GFW 污染（google→Facebook IP）
4. **dnsmasq 必须 `port=0`**：只做 DHCP，DNS 全权交给 sing-box（同上原因）
5. **DNS 服务器用隧道内 UDP**（8.8.8.8 detour hy2-out）而非直连 DoH——legacy 配置下 detour 行为不稳，
   隧道内 UDP 对 GFW 完全不可见；`strategy: ipv4_only` 防 AAAA 污染残留

## 运维

```bash
docker logs -f aipro-wifi-router      # 看日志（hostapd/分配/DNS）
docker restart aipro-wifi-router      # 重启（规则自动重建）
sudo bash run.sh                       # 全量重建
nmcli device set wlan1 managed no     # 若 NM 又抢 wlan1（持久配置已写入 conf.d）
# 改 SSID/密码/信道：hostapd.conf → docker restart
```

## 开机自愈（2026-08-15 重启实测通过）

黑名单已摘除（那是坏 cfg80211 时代的保守措施）。开机链全自动：
```
上电 → 绿联存储态(a69c:5724) → udev aic.rules 弹出 → bootrom(a69c:8d80)
→ modalias 自动加载 aic_load_fw → 固件下载 → 重枚举(368b:8d88)
→ modalias 自动加载 aic8800_fdrv → wlan1 → 容器(restart 策略)拉起 hostapd → AP
```
配套：wlan0 栈走 modules-load.d + NM 自动连 XinYuan；wlan1 持久 unmanaged。
真机重启验证：27s 内 wlan0 连接 + wlan1 AP + 容器 Up 全部就位。

## 已知限制 / 后续

- 信道宽度 20MHz（VHT80 参数对 FullMAC 驱动未生效，可再调）
- **分流已启用**（2026-08-15）：rule-set geosite-cn/geoip-cn → direct，DNS 分流
  （geosite-cn → 223.5.5.5 直连，其余 → 8.8.8.8 隧道内）；实测 baidu 0.04s 直连、
  google 0.16s 走代理、CN 出口=本地宽带、海外出口=hy2 服务器
- rule-set 首次从 raw.githubusercontent 下载（aipro 直连可达），cache_file 缓存后离线可用
- 5G 信道受世界域限制少（30/30 可用），2.4G 的 12/13 no-IR 不影响本 AP
- USB2 共享带宽，实测 ~800KB/s 下载（≈6.5Mbps）；上代理够用，大流量需换 USB3 网卡或有线
- gnp 宿主代理（:1080）已同步加同套分流规则；aipro 服务文件 wg 残留已清
