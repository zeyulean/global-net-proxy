# global-net-proxy 架构

## 一句话

国内外网络分流：**国外流量（github/google/代码源/AI）通过 mixed 代理端口走 Hysteria2 (QUIC) 隧道经 lwtop 海外出口，国内流量直连**。

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
  ┌──────────────────────────────┐   hysteria2 QUIC 隧道   ┌────────────────────────┐
  │  sing-box                     │  UDP 443 (QUIC/TLS 1.3) │  lwtop (8.209.203.17)  │
  │  ├─ mixed-in (0.0.0.0:1080)  │◀───────────────────────▶│  hy2-in (gnp-hy2)      │
  │  │   socks5 + http 代理       │                          │  ├─ NAT 转发           │
  │  ├─ DNS 分流                  │                          │  └─ 出口: 海外公网     │
  │  │   国内→223.5.5.5 直连      │                          └────────────────────────┘
  │  │   国外→1.1.1.1 TCP 走 hy2  │
  │  ├─ hy2 outbound (hy2-out)    │
  │  │   password 认证 + TLS      │
  │  └─ route 规则                │
  │       ip_is_private → direct  │
  │       geosite 国外 → hy2-out  │
  │       geosite-cn → direct     │
  │       final → hy2-out         │
  └──────────────────────────────┘
```

## 分流原则

| 流量 | 判定规则 | 出口 |
|------|---------|------|
| github/google/openai/pypi/npm/crates/go/maven/docker 等 | geosite 命中 | hy2 → lwtop 海外 |
| 国内域名/国内 IP | geosite-cn / geoip-cn 命中 | direct 直连 |
| 私有 IP (10.x/172.16.x/192.168.x) | ip_is_private | direct 直连 |
| 其余未知 | final=hy2-out | 走 hy2 |

## 组件分工

| 组件 | 端 | 作用 |
|------|----|----|
| `gnp-hy2` (sing-box systemd 服务) | **server** (lwtop) | hysteria2 入站 + 密码认证 + NAT 转发 |
| `sing-box` (二进制, with_quic) | **client** | mixed 代理端口 + DNS 分流 + hy2 outbound + route 规则 |
| geosite/geoip 规则集 | client | 判断国内外域名的依据，remote rule-set 自动更新 |
| `gnp-client` (Rust CLI) | **client** | 管理 sing-box (start/stop/status/wg/config/test) |
| `gnp-server` (Rust CLI) | **server** | 管理 hy2 server (install/users/add-user/pregen/activate) |

## Server 部署要点 (gnp-hy2)

- **配置**：`/opt/gnp-quic/config.json`（sing-box hysteria2 inbound）
- **服务**：`/opt/gnp-quic/sing-box` 二进制 + systemd 服务 `gnp-hy2`
- **自签证书**：`/opt/gnp-quic/certs/server.crt` / `server.key`（openssl 生成，10 年有效期）
- **端口**：`443`（UDP/QUIC），阿里云安全组放行 UDP 443
- **认证**：`users` 数组存放密码（`gnp-server add-user` / `pregen` / `activate` 管理）
- **用户池**：`/opt/gnp-quic/pending-users/*.json`（预生成的密码包）

server 配置片段：

```json
{
  "inbounds": [{
    "type": "hysteria2",
    "tag": "hy2-in",
    "listen": "::",
    "listen_port": 443,
    "users": [ { "password": "<HY2_PASSWORD>" } ],
    "tls": {
      "enabled": true,
      "certificate_path": "/opt/gnp-quic/certs/server.crt",
      "key_path": "/opt/gnp-quic/certs/server.key"
    }
  }]
}
```

## 客户端配置要点 (hysteria2 outbound)

sing-box **1.13.16**（with_quic 构建，原生支持 hysteria2 outbound）。

1. **hysteria2 outbound**：
   - 用 `outbounds` 数组，`type: "hysteria2"`
   - 认证用 `password` 字段（无需公钥对）
   - `tls: { enabled: true, insecure: true }`（信任自签证书）
   - `server_port: 443`（QUIC/UDP）

2. **DNS server**：
   - 国外域名经 hy2 走 `1.1.1.1` TCP（`detour: hy2-out`）
   - 国内域名走 `223.5.5.5` 直连（`detour: direct`）

3. **mixed inbound**：
   - `type: "mixed"` 同时支持 socks5 和 http 代理
   - `listen: "0.0.0.0"` 允许局域网设备使用
   - `listen_port: 1080`

4. **route.final = hy2-out**（非 direct）：
   - 未匹配的域名默认走代理（安全默认值，访问国外不会被墙）

5. **systemd service**：
   - 系统级 `/etc/systemd/system/gnp-proxy.service`

## 网络细节

- 隧道协议：`hysteria2`（QUIC over UDP 443，TLS 1.3 加密）
- server 端口：`443`（UDP，阿里云安全组放行）
- 代理端口：`1080`（mixed: socks5 + http）
- sing-box 版本：`1.13.16`（with_quic，原生 hysteria2 outbound）
- 认证：Hysteria2 密码（`HY2_PASSWORD`）
- DNS：国内 `223.5.5.5`（阿里 UDP 直连），国外 `1.1.1.1` TCP（Cloudflare，走 hy2）
- 无 MTU 调整需求（QUIC 自适应分片，无需 wg 的 1280 MTU）

## 目录结构

```
global-net-proxy/
├── Cargo.toml              # Cargo workspace 根
├── crates/
│   ├── gnp-core/           # 共享库: 平台/config/hy2 诊断/服务管理
│   ├── gnp-client/         # client CLI (install/start/stop/status/wg/config/test/register/update-rules/cleanup/recover/proxy)
│   └── gnp-server/         # server CLI (install/uninstall/status/users/add-user/pregen/activate)
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
│   └── safe-template.json  # 安全配置模板 (mixed + hysteria2 outbound 格式)
├── docs/
│   ├── architecture.md     # 本文档
│   ├── setup.md            # 各端安装指南
│   ├── auto-registration.md# 自动注册方案
│   └── incident-2026-08-10.md # aipro 断网事故记录
└── peers/
    └── HY2_PASSWORD        # server 密码校验值 (可公开)
    # slot-*.json           # 用户池 (含密码), 只存 gitee 私有仓库, 不进公开 repo
```

> `bash/` 仅保留应急兜底脚本（断网时 Rust 二进制无法运行）。正式管理使用 Rust CLI (`gnp-client` / `gnp-server`)。