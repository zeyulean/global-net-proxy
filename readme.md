# global-net-proxy

国内外网络分流工具：**国外流量（github / google / 代码源 / AI API）走 Hysteria2 (QUIC) 隧道经海外出口，国内流量直连**。

客户端使用 sing-box **mixed 代理模式**（socks5+http 端口 1080），不碰路由表，零断网风险。

## 特性

- 🚀 **国外走隧道**：github / google / openai / pypi / npm / crates / go / maven / docker 等自动走 hy2
- 🇨🇳 **国内直连**：国内域名 / IP 自动识别，不绕行
- 🔄 **规则自动更新**：geosite/geoip 规则集每 24h 自动更新（sing-box remote rule-set）
- 🔒 **Hysteria2 (QUIC) 加密**：基于 QUIC/TLS 1.3，抗封锁，UDP 443 端口
- 🔑 **密码认证**：server 端密码池（HY2_PASSWORD），替代 wg 公钥对
- 🔒 **mixed 代理模式（安全）**：只开 socks5+http 端口 1080，**绝不使用 tun 模式**
- 💻 **Rust CLI**：client + server 统一管理，跨平台（macOS launchd / Linux systemd）
- 📦 **sing-box 1.13.16**：使用 outbound hysteria2 格式（with_quic 构建）

## CLI 代理（curl/pip/uv/npm/git 等）

CLI 工具不读系统代理设置，只认环境变量。`gnp-client env` 子命令解决：

```bash
eval "$(gnp-client env --on)"    # 当前 shell 立即生效（curl/pip/npm/git 走 127.0.0.1:1080）
eval "$(gnp-client env --off)"   # 取消
eval "$(gnp-client env --hook)"  # 写入 .zshrc/.bashrc → 得到 gnp-on / gnp-off 快捷函数
gnp-client env                   # 查看当前 shell 代理状态
```

- 国内流量**无需**配 no_proxy 白名单：sing-box 内部已按 geosite-cn 分流直连
  （实测 baidu 0.05s 直连 / google 0.19s 走 hy2）
- Windows PowerShell 等价：`$env:https_proxy='http://127.0.0.1:1080'; $env:http_proxy=$env:https_proxy`
- GUI/浏览器走 `gnp-client proxy --on`（系统代理），CLI 走 `env --on`，两套互不干扰

## ⚠️ 安全原则

> **绝不在无带外访问的机器上使用 tun 模式。**
> tun 的 `strict_route` + `auto_route` 会接管系统路由表，一旦配置有误会导致完全断网。
> mixed 代理模式只监听端口，不碰路由表，是安全替代方案。
> 详见 [aipro 断网事故记录](docs/incident-2026-08-10.md)。

## 架构

```
客户端(sing-box mixed:1080) ──hysteria2 QUIC 隧道──▶ server(gnp-hy2, lwtop海外) ──▶ 国外目标
     │
     └──国内域名/IP──▶ 直连 (不经过代理)
```

详见 [docs/architecture.md](docs/architecture.md) | **[docs/usage.md](docs/usage.md)（完整使用手册）**

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
sudo gnp-server install          # 安装 sing-box + 自签证书 + QUIC + 开机自启
sudo gnp-server status           # 查看状态
sudo gnp-server add-user macbook # 添加客户端（生成密码）
sudo gnp-server pregen 20        # 预生成 20 个用户密码池
sudo gnp-server activate <id>    # 激活预生成的用户
```

### 2. 部署 client（Mac/Ubuntu）

```bash
# 方式 A: 从 gitee 自动注册（推荐，一键完成）
export GITEE_TOKEN=xxxx
gnp-client register my-client-id
# 自动: 拉取用户密码 → 生成 config → 安装 sing-box

# 方式 B: 手动安装
gnp-client install \
  --server 8.209.203.17 \
  --password <HY2_PASSWORD> \
  --server-port 443

gnp-client start    # 启动 sing-box 代理（开机自启）
gnp-client stop     # 停止
gnp-client status   # 查看状态（进程/端口/隧道/出口IP）
gnp-client wg       # 隧道诊断（命令名保留，实际诊断 hy2）
gnp-client config --check  # 校验配置安全
gnp-client test     # 测试代理连通性
gnp-client update-rules --install-cron  # 每日自动更新规则
```

### 3. 使用代理

#### macOS（系统代理）

```bash
gnp-client proxy --on       # 开启系统代理 (osascript 弹授权, 不存密码)
gnp-client proxy --status   # 查看代理状态
gnp-client proxy --off      # 关闭系统代理
```

> 开启后 Safari / Chrome 等浏览器自动走 sing-box 代理。

#### Linux（环境变量 / GNOME）

```bash
# 方式 A: 环境变量（终端程序）
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080
export all_proxy=socks5://127.0.0.1:1080

# 方式 B: GNOME 系统代理
gnp-client proxy --on

# 或单次
curl -x socks5h://127.0.0.1:1080 https://www.google.com
```

> 💡 完整命令说明请参阅 [docs/usage.md](docs/usage.md)

## 应急脚本

`bash/client/` 目录保留两个断网应急脚本（断网时 Rust 二进制无法运行）：
- `cleanup-aipro.sh` — 应急清理
- `recover-aipro-network.sh` — 断网恢复

## Server 信息（公开）

| 项目 | 值 |
|------|------|
| Server 地址 | `8.209.203.17:443`（UDP/QUIC） |
| 认证方式 | Hysteria2 密码（`HY2_PASSWORD`，由 `gnp-server add-user` 生成） |
| 证书 | 自签证书 `/opt/gnp-quic/certs/`（客户端 `insecure: true` 信任） |

> 密码需要安全传输，不影响 server 安全性。密码池存在 gitee 私有仓库。

## 部署矩阵（2026-08-15 实体部署已收敛）

| 节点 | 角色 | 二进制（实体拷贝） | 服务 | 实例 |
|---|---|---|---|---|
| Mac | client | `~/.local/bin/gnp-client` | launchd com.gnp.sing-box | 单一 ✓ |
| aipro | client + 无线路由 | `~/.local/bin/gnp-client` | systemd gnp-proxy | 单一 ✓（路由容器内 sing-box 为独立角色） |
| lwtop | server | `/usr/local/bin/gnp-server` | systemd gnp-hy2 | 单一 ✓ (UDP 443) |
| vmwin (Win11 ARM64) | client ✨ | `%USERPROFILE%\.local\bin\gnp-client.exe` | 计划任务 gnp-singbox (ONLOGON) | ✅ 实测: 出口8.209.203.17, proxy on/off 回合通过 (x64 模拟层) |

### Windows 支持（2026-08-15）

- 命令面与 mac/linux 一致：install/start/stop/status/config/test/tunnel/proxy
- 服务：schtasks 计划任务 `gnp-singbox`（当前用户登录自启，**无需管理员**）
- 系统代理：写 HKCU WinINET 注册表 + InternetSetOption 刷新（浏览器即时生效）
- 数据目录同为 `~\.local\share\sing-box\`（三端路径统一）
- cleanup/recover 不适用（无 tun 路由风险），会给出说明
- 交叉编译：`rustup target add x86_64-pc-windows-gnu && brew install mingw-w64 &&
  cargo build -p gnp-client --release --target x86_64-pc-windows-gnu`
- vmwin 为 ARM64 Win11，x64 exe 走系统模拟层运行良好（sing-box with_quic ✓）
- ⚠️ sing-box ≥1.13 兼容三连（generator 已修）：DNS 新格式（type/server 字段）、
  route.default_domain_resolver、规则下载失败不得污染已有 .srs（tmp+rename）

### 部署规范（必须遵守）

1. **仓库与部署职责分离**：repo 只管源码/文档；部署物必须是**实体拷贝**
   （`cp` 到 `~/.local/bin/`、`/usr/local/bin/`），**禁止 symlink 指向仓库**
   ——仓库移动/清理会瞬间打断生产。
2. **单实例纪律**：更新 = 停服务 → 拷贝 → 启动 → `pgrep -c` 核对唯一；
   手动测试跑过 binary 后必须确认没有逃逸进程占端口（aipro 曾因此 crash-loop 111 次）。
3. **跨机传二进制先 `file` 核架构**（Mac arm64 Mach-O ≠ Linux x86_64 ELF，
   2026-08-15 曾差点把 Mach-O 覆盖到 lwtop）。远端有 cargo 时优先远端本地构建。
4. systemd 服务带 `cache_file` 时必须给**绝对路径**（默认 CWD=/ 不可写 → 启动即死）。

> 全线 CN 分流：geosite/geoip-cn 直连，国外走 hy2/QUIC。wg 时代代码已清
> （`tunnel` 为主命令，`wg` 保留别名；运行配置纯 hysteria2 outbound）。

## 目录结构

```
├── Cargo.toml              # Cargo workspace 根
├── crates/
│   ├── gnp-core/           # 共享库: 平台/config/hy2 诊断/服务管理
│   ├── gnp-client/         # client CLI (install/start/stop/status/tunnel/config/test/register/update-rules/cleanup/recover/proxy)
│   └── gnp-server/         # server CLI (install/uninstall/status/users/add-user/pregen/activate)
├── vendor/
│   └── sing-box/           # submodule: sing-box 源码
├── aipro-wifi/             # 子项目: OrangePi AIpro WiFi 修复 + 无线路由 docker
│   ├── artifacts/          #   可部署 .ko 三件套 + 恢复脚本 (分钟级重建)
│   ├── router-docker/      #   无线路由容器: hostapd+dnsmasq+sing-box tproxy (连 SSID aipro 即出海)
│   ├── docs/01-06          #   三层根因链 + 21 轮实验翻案全记录
│   └── resources/          #   submodule → 分支 aipro-resources (sing-box 二进制 + aic 源码)
├── bin/                    # 构建产物 (gitignore)
├── bash/
│   ├── install.sh          # 构建 + 安装所有依赖到安装目录
│   ├── uninstall.sh        # 卸载
│   └── client/             # (应急) 断网恢复/清理 bash 兜底脚本
├── config/                 # 安全配置模板 (mixed + hysteria2 outbound)
├── docs/                   # 架构 + 安装指南 + 自动注册 + 事故记录
└── peers/                  # server 密码校验文件 + 用户池（gitee 私有）
    └── HY2_PASSWORD        # server 密码校验值（可公开）
    # slot-*.json           # 用户池（含密码），只存 gitee 私有仓库，不进公开 repo
```

## 仓库

- main: github
- backup: gitee （双远端同步）

## License

MIT