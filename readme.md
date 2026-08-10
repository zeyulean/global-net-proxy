# global-net-proxy

国内外网络分流工具：**国外流量（github / google / 代码源 / AI API）走 WireGuard 隧道经海外出口，国内流量直连**。

客户端使用 sing-box **mixed 代理模式**（socks5+http 端口 1080），不碰路由表，零断网风险。

## 特性

- 🚀 **国外走隧道**：github / google / openai / pypi / npm / crates / go / maven / docker 等自动走 wg
- 🇨🇳 **国内直连**：国内域名 / IP 自动识别，不绕行
- 🔄 **规则自动更新**：geosite/geoip 规则集每 24h 自动更新（sing-box remote rule-set）
- 🛡️ **WireGuard 加密**：userspace 实现（system:false），不需要内核模块，不需要 root
- 🔒 **mixed 代理模式（安全）**：只开 socks5+http 端口 1080，**绝不使用 tun 模式**
- 💻 **跨平台 client**：Ubuntu / macOS / Windows 一套脚本

## ⚠️ 安全原则

> **绝不在无带外访问的机器上使用 tun 模式。**
> tun 的 `strict_route` + `auto_route` 会接管系统路由表，一旦配置有误会导致完全断网。
> mixed 代理模式只监听端口，不碰路由表，是安全替代方案。
> 详见 [aipro 断网事故记录](docs/incident-2026-08-10.md)。

## 架构

```
客户端(sing-box mixed:1080) ──wg隧道──▶ server(wg-quick, lwtop海外) ──▶ 国外目标
     │
     └──国内域名/IP──▶ 直连 (不经过代理)
```

详见 [docs/architecture.md](docs/architecture.md)

## 快速开始

```bash
# 1. 部署 server (lwtop/Ubuntu)
scp bash/server/server.sh lw:~/
sudo bash server.sh --install
sudo bash server.sh --add-peer macbook

# 2. 部署 client (Mac/Ubuntu/Windows)
bash client.sh --install       # 交互式安装
bash client.sh --test          # 先跑 10 秒测试
bash client.sh --start         # 启动

# 3. 使用代理
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080
```

详见 [docs/setup.md](docs/setup.md)

## 目录

```
bash/server/        wg server 一键管理 (Ubuntu)
bash/client/        sing-box client 跨平台管理 (mixed 模式)
config/             安全配置模板
docs/               架构 + 安装指南 + 事故记录
```

## 仓库

- main: github
- backup: gitee （双远端同步）

## License

MIT
