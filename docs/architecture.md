# global-net-proxy 架构

## 一句话

国内外网络分流：**国外流量（github/google/代码源/AI）通过 mixed 代理端口走 WireGuard 隧道经 lwtop 海外出口，国内流量直连**。

## ⚠️ 安全原则

### 为什么不用 tun 模式

tun 模式（`strict_route: true` + `auto_route: true`）会让 sing-box **接管系统路由表**。这在以下场景中是灾难性的：

- **无带外访问的机器**：一旦路由表被破坏，SSH 完全不通，无法远程修复
- **systemd 开机自启**：重启不会拯救你——每次启动都会再次破坏路由
- **User=root 服务**：即使用户账户失效，root 服务仍会执行

**2026-08-10 aipro 断网事故**就是 tun 模式导致的——sing-box 接管了 ARM 开发板的路由表，SSH 完全不通，最终需要拔 TF 卡到另一台机器上修复。详见 [incident-2026-08-10.md](incident-2026-08-10.md)。

### mixed 代理模式是安全替代

| 对比项 | tun 模式（危险） | mixed 模式（安全） |
|--------|-----------------|-------------------|
| 路由表 | `strict_route` + `auto_route` 接管 | **完全不碰** |
| 权限 | 需要 root | **普通用户即可** |
| 断网风险 | 高（路由被接管后 SSH 不通） | **零**（只开代理端口） |
| 透明代理 | 是（系统级） | 否（需设置 http_proxy） |
| 使用方式 | 无感 | `export http_proxy=http://127.0.0.1:1080` |

mixed 模式只监听 `0.0.0.0:1080`（同时支持 socks5 和 http 代理），不创建 tun 设备，不修改路由表。即使配置错误，也不会影响系统网络。

## 拓扑

```
  客户端 (Mac / Ubuntu / Windows)
  ┌──────────────────────────────┐         wg 隧道          ┌────────────────────────┐
  │  sing-box                     │  UDP 51820 (加密)        │  lwtop (8.209.203.17)  │
  │  ├─ mixed-in (0.0.0.0:1080)  │◀────────────────────────▶│  wg server (wg-quick)   │
  │  │   socks5 + http 代理       │                          │  ├─ NAT/SNAT 转发        │
  │  ├─ DNS 分流                  │                          │  └─ 出口: 海外公网       │
  │  │   国内→223.5.5.5 直连      │                          └────────────────────────┘
  │  │   国外→1.1.1.1 走wg        │
  │  ├─ wg endpoint (wg-ep)       │
  │  │   system:false (userspace) │
  │  └─ route 规则                │
  │       ip_is_private → direct  │
  │       geosite 国外 → wg-ep    │
  │       geosite-cn → direct     │
  │       final → wg-ep           │
  └──────────────────────────────┘
```

## 分流原则

| 流量 | 判定规则 | 出口 |
|------|---------|------|
| github/google/openai/pypi/npm/crates/go/maven/docker 等 | geosite 命中 | wg → lwtop 海外 |
| 国内域名/国内 IP | geosite-cn / geoip-cn 命中 | direct 直连 |
| 私有 IP (10.x/172.16.x/192.168.x) | ip_is_private | direct 直连 |
| 其余未知 | final=wg-ep | 走 wg |

## 组件分工

| 组件 | 端 | 作用 |
|------|----|----|
| `wg-quick` (原生 wireguard-tools) | **server** (lwtop) | wg server + NAT 转发 |
| `sing-box` (二进制) | **client** | mixed 代理端口 + DNS 分流 + wg endpoint (userspace) + route 规则 |
| geosite/geoip 规则集 | client | 判断国内外域名的依据，remote rule-set 自动更新 |
| `gnp-client` (Rust CLI) | **client** | 管理 sing-box (start/stop/status/wg/config/test) |
| `gnp-server` (Rust CLI) | **server** | 管理 wg server (install/peers/add-peer/pregen/activate) |

## sing-box 配置要点 (1.12+)

> **重要**: sing-box 1.11→1.12 有破坏性变更，以下格式要求适用于 1.12+（当前 1.13.16）

1. **WireGuard endpoint**（不是 outbound）：
   - 用 `endpoints` 数组，`type: "wireguard"`
   - peer 用 `address` + `port` 字段（不是旧 `server` + `server_port`）
   - `system: false` 使用 userspace wireguard（不需要内核模块，不需要 root）

2. **DNS server**（新格式）：
   - 用 `type: "https"` 或 `type: "udp"` + `server` 字段
   - **不是**旧 `address` 字段

3. **ruleset download_detour 用 direct**：
   - 如果用 `wg-ep` 下载 ruleset，存在鸡蛋问题（wg 需要解析域名→需要 DNS→DNS 可能依赖 wg）
   - 用 `direct` 直连下载，避免循环依赖

4. **mixed inbound**：
   - `type: "mixed"` 同时支持 socks5 和 http 代理
   - `listen: "0.0.0.0"` 允许局域网设备使用
   - `listen_port: 1080`

5. **route.final = wg-ep**（非 direct）：
   - 未匹配的域名默认走代理（安全默认值，访问国外不会被墙）

## 网络细节

- wg 隧道网段：`10.0.0.0/24`（server=`10.0.0.1/24`，客户端从 `.2` 递增）
- wg 端口：`51820`（UDP）
- 代理端口：`1080`（mixed: socks5 + http）
- DNS：国内 `223.5.5.5`（阿里 UDP），国外 `https://1.1.1.1`（Cloudflare DoH，走 wg）
- MTU：1280（wg 隧道，避免分片）

## 目录结构

```
global-net-proxy/
├── Cargo.toml              # Cargo workspace 根
├── crates/
│   ├── gnp-core/           # 共享库: 平台/config/wg 诊断/服务管理
│   ├── gnp-client/         # client CLI (install/start/stop/status/wg/config/test/register/update-rules/cleanup/recover)
│   └── gnp-server/         # server CLI (install/uninstall/status/peers/add-peer/pregen/activate)
├── vendor/
│   └── sing-box/           # submodule: sing-box 源码
├── bin/                    # 构建产物 (gitignore)
├── bash/
│   ├── install.sh          # 构建 + 安装所有依赖到安装目录
│   ├── uninstall.sh        # 卸载
│   └── client/
│       ├── cleanup-aipro.sh    # (应急) aipro 事故清理兜底
│       └── recover-aipro-network.sh  # (应急) 断网恢复兜底
├── config/
│   └── safe-template.json  # 安全配置模板 (mixed + wg endpoint)
├── docs/
│   ├── architecture.md     # 本文档
│   ├── setup.md            # 各端安装指南
│   ├── auto-registration.md# 自动注册方案
│   └── incident-2026-08-10.md # aipro 断网事故记录
└── peers/
    └── SERVER_PUBKEY       # server 公钥 (可公开)
    # slot-*.json           # peer 池 (含私钥), 只存 gitee 私有仓库, 不进公开 repo
```

> `bash/` 仅保留应急兜底脚本（断网时 Rust 二进制无法运行）。正式管理使用 Rust CLI (`gnp-client` / `gnp-server`)。
