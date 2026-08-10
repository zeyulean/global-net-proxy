# global-net-proxy

国内外网络分流工具：**国外流量（github / google / 代码源 / AI API）走 WireGuard 隧道经海外出口，国内流量直连**。

WireGuard 隧道提供加密、低延迟、稳定的传输；客户端用 sing-box 做按域名分流，无需手动维护 IP 白名单。

## 特性

- 🚀 **国外走隧道**：github / google / openai / pypi / npm / crates / go / maven / docker 等自动走 wg
- 🇨🇳 **国内直连**：国内域名 / IP 自动识别，不绕行
- 🔄 **规则自动更新**：geosite/geoip 规则集每 24h 自动更新（sing-box remote rule-set）
- 🛡️ **WireGuard 加密**：内核态、低延迟、稳定
- 💻 **跨平台 client**：Ubuntu / macOS / Windows 一套脚本
- 🖥️ **server only Ubuntu**：原生 wg-quick，成熟可靠

## 架构

```
客户端(sing-box) ──wg隧道──▶ server(wg-quick, lwtop海外) ──▶ 国外目标
     │
     └──国内域名──▶ 直连
```

详见 [docs/architecture.md](docs/architecture.md)

## 快速开始

```bash
# 1. 部署 server (lwtop/Ubuntu)
scp bash/server/server.sh lw:~/
sudo bash server.sh --install
sudo bash server.sh --add-peer macbook

# 2. 部署 client (Mac/Ubuntu/Windows)
bash client.sh --install
bash client.sh --start
```

详见 [docs/setup.md](docs/setup.md)

## 目录

```
bash/server/    wg server 一键管理 (Ubuntu)
bash/client/    sing-box client 跨平台管理
docs/           架构 + 安装指南
```

## 仓库

- main: github
- backup: gitee （双远端同步）

## License

MIT