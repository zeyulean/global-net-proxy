# global-net-proxy 架构

## 一句话

国内外网络分流：**国外流量（github/google/代码源/AI）走 WireGuard 隧道经 lwtop 海外出口，国内流量直连**。

## 拓扑

```
  客户端 (Mac / Ubuntu / Windows)
  ┌──────────────────────────────┐         wg 隧道          ┌────────────────────────┐
  │  sing-box                     │  UDP 51820 (加密)        │  lwtop (8.209.203.17)  │
  │  ├─ tun-in  (接管系统流量)    │◀────────────────────────▶│  wg server (wg-quick)   │
  │  ├─ DNS 分流                  │                          │  ├─ NAT/SNAT 转发        │
  │  │   国内→223.5.5.5 直连      │                          │  └─ 出口: 海外公网       │
  │  │   国外→1.1.1.1 走wg        │                          └────────────────────────┘
  │  ├─ wg endpoint (wg-out)      │
  │  └─ route 规则                │
  │       国外 geosite → wg-out   │
  │       国内 geoip/geosite → direct
  └──────────────────────────────┘
```

## 分流原则

| 流量 | 判定规则 | 出口 |
|------|---------|------|
| github/google/openai/pypi/npm/crates/go/maven/docker 等 | geosite 命中 | wg → lwtop 海外 |
| 国内域名/国内 IP | geosite-cn / geoip-cn 命中 | direct 直连 |
| 其余未知 | final=direct | 直连 |

## 组件分工

| 组件 | 端 | 作用 |
|------|----|----|
| `wg-quick` (原生 wireguard-tools) | **server** (lwtop) | wg server + NAT 转发。sing-box 只能做 client，不能做 server，故 server 用原生 wg |
| `sing-box` (二进制) | **client** | tun 接管流量 + DNS 分流 + wg endpoint + route 规则 |
| geosite/geoip 规则集 | client | 判断国内外域名的依据，remote rule-set 自动更新 |

## 为什么 sing-box 做 client

sing-box 的 WireGuard 是 **endpoint**（1.11+ 重构），只能连接远端 wg server，**不能反向做 server**。所以：
- server 端 = 原生 `wg-quick`（成熟、稳定、内核态）
- client 端 = sing-box（提供 tun 接管 + 规则分流能力，原生 wg-quick 做不到按域名分流）

## 规则集自动更新

- sing-box 的 `route.rule_set` 用 `type: remote`，**启动时下载 + 默认每 24h 自动更新**
- 数据源：`lyc8503/sing-box-rules`（GitHub Actions 每日同步上游 v2ray-rules-dat）
- cron 兜底：`update-rules.sh --check` 每天检查 sing-box 常驻，挂了自动重启

## 网络细节

- wg 隧道网段：`10.99.0.0/24`（server=`10.99.0.1/24`，客户端从 `.2` 递增）
- wg 端口：`51820`（UDP）
- client tun 网段：`172.19.0.1/30`（sing-box 内部）
- DNS：国内 `223.5.5.5`（阿里），国外 `https://1.1.1.1/dns-query`（Cloudflare DoH，走 wg）

## 国家/区域判断的准确性

- geosite-cn / geoip-cn 来自 v2ray-rules-dat 每日同步，覆盖国内主流域名与 IP 段
- 国外目标分组（google/github/openai/...）为精确白名单式 geosite 分类
- 未知域名走 final=direct 直连，避免误伤

## 目录结构

```
global-net-proxy/
├── bash/
│   ├── server/
│   │   └── server.sh          # wg server 一键管理 (Ubuntu)
│   └── client/
│       ├── client.sh          # sing-box client 跨平台管理
│       └── update-rules.sh    # 规则更新 + 常驻检查 + cron
├── docs/
│   ├── architecture.md        # 本文档
│   └── setup.md               # 各端安装指南
└── readme.md
```