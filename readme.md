# global-net-proxy

国内外网络分流工具：**国外流量（github / google / 代码源 / AI API）走 WireGuard 隧道经海外出口，国内流量直连**。

客户端使用 sing-box **mixed 代理模式**（socks5+http 端口 1080），不碰路由表，零断网风险。

## 特性

- 🚀 **国外走隧道**：github / google / openai / pypi / npm / crates / go / maven / docker 等自动走 wg
- 🇨🇳 **国内直连**：国内域名 / IP 自动识别，不绕行
- 🔄 **规则自动更新**：geosite/geoip 规则集每 24h 自动更新（sing-box remote rule-set）
- 🛡️ **WireGuard 加密**：userspace 实现（system:false），不需要内核模块，不需要 root
- 🔒 **mixed 代理模式（安全）**：只开 socks5+http 端口 1080，**绝不使用 tun 模式**
- 💻 **Rust CLI**：client + server 统一管理，跨平台（macOS launchd / Linux systemd）

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

## 快速开始（Rust CLI）

### 0. 安装 gnp CLI

```bash
# 构建 + 安装到系统 PATH
bash bash/install.sh

# 验证
gnp-client --version   # gnp-client 0.1.0
gnp-server --version   # gnp-server 0.1.0

# 卸载
bash bash/uninstall.sh           # 移除软链
bash bash/uninstall.sh --clean   # 移除软链 + 删除 bin/ 产物
```

### 1. 部署 server（lwtop/Ubuntu）

```bash
sudo gnp-server install          # 安装 wg + NAT + 开机自启
sudo gnp-server status           # 查看状态
sudo gnp-server add-peer macbook # 添加客户端
sudo gnp-server pregen 20        # 预生成 20 个 peer 池
sudo gnp-server activate <id>    # 激活预生成的 peer
```

### 2. 部署 client（Mac/Ubuntu）

```bash
# 方式 A: 从 gitee 自动注册（推荐，一键完成）
export GITEE_TOKEN=xxxx
gnp-client register my-client-id
# 自动: 拉取 peer → 生成 config → 安装 sing-box

# 方式 B: 手动安装
gnp-client install \
  --server 8.209.203.17 \
  --server-pubkey <pubkey> \
  --client-privkey <privkey> \
  --client-ip 10.0.0.5/32

gnp-client start    # 启动 sing-box 代理（开机自启）
gnp-client stop     # 停止
gnp-client status   # 查看状态（进程/端口/隧道/出口IP）
gnp-client wg       # wg 隧道诊断
gnp-client config --check  # 校验配置安全
gnp-client test     # 测试代理连通性
gnp-client update-rules --install-cron  # 每日自动更新规则
```

### 3. 使用代理

```bash
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080
export all_proxy=socks5://127.0.0.1:1080

# 或单次
curl -x socks5://127.0.0.1:1080 https://www.google.com
```

## 应急脚本

`bash/client/` 目录保留两个断网应急脚本（断网时 Rust 二进制无法运行）：
- `cleanup-aipro.sh` — 应急清理
- `recover-aipro-network.sh` — 断网恢复

## Server 信息（公开）

| 项目 | 值 |
|------|------|
| Server 地址 | `8.209.203.17:51820` |
| Server 公钥 | `M/t3YYwIW7Xou+vASGtNoAHHNrh82ROzYDU4LIsLz18=` |
| Client 网段 | `10.0.0.2` – `10.0.0.250` |

> Server 公钥可以公开，不影响安全性。Client 私钥存在 gitee 私有仓库。

## 目录结构

```
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
│   └── client/             # (应急) 断网恢复/清理 bash 兜底脚本
├── config/                 # 安全配置模板
├── docs/                   # 架构 + 安装指南 + 自动注册 + 事故记录
└── peers/                  # server 公钥 + peer 池（gitee 私有）
    └── SERVER_PUBKEY       # server 公钥（可公开）
    # slot-*.json           # peer 池（含私钥），只存 gitee 私有仓库，不进公开 repo
```

## 仓库

- main: github
- backup: gitee （双远端同步）

## License

MIT