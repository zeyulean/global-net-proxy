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
- 💻 **Rust CLI 三平台**：macOS (launchd) / Linux (systemd) / Windows (schtasks)
- 🤝 **一键接入**：`gnps gen-user` 生成 gnp.cfg → 客户端 `gnpc peer gnp.cfg` 完成
- 📦 **sing-box 1.13.16**：使用 outbound hysteria2 格式（with_quic 构建）

## CLI 代理（curl/pip/uv/npm/git 等）

CLI 工具不读系统代理设置，只认环境变量。`gnpc env` 子命令解决：

```bash
eval "$(gnpc env --on)"    # 当前 shell 立即生效（curl/pip/npm/git 走 127.0.0.1:1080）
eval "$(gnpc env --off)"   # 取消
eval "$(gnpc env --hook)"  # 写入 .zshrc/.bashrc → 得到 gnp-on / gnp-off 快捷函数
gnpc env                   # 查看当前 shell 代理状态
```

- 国内流量**无需**配 no_proxy 白名单：sing-box 内部已按 geosite-cn 分流直连
  （实测 baidu 0.05s 直连 / google 0.19s 走 hy2）
- Windows PowerShell 等价：`$env:https_proxy='http://127.0.0.1:1080'; $env:http_proxy=$env:https_proxy`
- GUI/浏览器走 `gnpc proxy --on`（系统代理），CLI 走 `env --on`，两套互不干扰

## ⚠️ 安全原则

> **绝不在无带外访问的机器上使用 tun 模式。**
> tun 的 `strict_route` + `auto_route` 会接管系统路由表，一旦配置有误会导致完全断网。
> mixed 代理模式只监听端口，不碰路由表，是安全替代方案。
> 详见 [aipro 断网事故记录](docs/incident-2026-08-10.md) 与
> [2026-08-15 分流故障全档案](docs/incident-2026-08-15-dns.md)（DNS 污染/rule-set 阻塞/hijack 迟滞/cache 路径四层根因）。

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
gnpc --version   # gnpc 0.1.0
gnps --version   # gnps 0.1.0

# 卸载
bash bash/uninstall.sh           # 移除软链
bash bash/uninstall.sh --clean   # 移除软链 + 删除 bin/ 产物
```

### 1. 部署 server（lwtop/Ubuntu）

```bash
# 事实源: deploy/hosts/lwtop-server.toml → /opt/gnp/config.toml (唯一事实源)
scp deploy/hosts/lwtop-server.toml lwtop:/tmp/
ssh lwtop 'sudo install -m 600 /tmp/lwtop-server.toml /opt/gnp/config.toml
           sudo /opt/gnp/bin/gnps install --config /opt/gnp/config.toml'
# gnps install = 渲染 config.json + sing-box check + gnps.service + tick 调度 + 开机自启

sudo /opt/gnp/bin/gnps status           # gnps active + UDP 5766 监听
sudo /opt/gnp/bin/gnps gen-user --name macbook  # 加用户 + 出 gnp.cfg (含 5766 + obfs)
sudo /opt/gnp/bin/gnps users            # 列用户
sudo /opt/gnp/bin/gnps pregen 20        # 预生成 20 个用户密码池
sudo /opt/gnp/bin/gnps activate <id>    # 激活预生成的用户
```

### 2. 部署 client（Mac/Ubuntu）

```bash
# 方式 A: 新机器 (从零)
gnpc init --server 8.209.203.17 --hy2-password <密码> --obfs-password <obfs> \
          --listen 127.0.0.1     # 生成唯一事实源 config.toml
gnpc install                      # 下载 sing-box + 规则集, 渲染 config.json, 装常驻服务
gnpc install-scheduler            # 装调度 (tick.sh + launchd/crontab), 并清旧
gnpc start

# 方式 B: 从旧布局迁移 (推荐用于存量机器, 幂等可重跑)
scp deploy/hosts/<host>.toml <host>:~/.local/gnp/config.toml
scp gnpc <host>:/tmp/ && ssh <host> '/tmp/gnpc migrate --config ~/.local/gnp/config.toml'
# migrate = 落事实源 → 搬资产 → sing-box check → 停旧让位端口 → 装新调度
#          → 验进程+端口+出口 IP → 才归档旧目录; 任一步失败自动装回旧服务

# 方式 C: 从 gitee 自动注册
export GITEE_TOKEN=xxxx
gnpc register my-client-id

gnpc start    # 启动 sing-box 代理（开机自启）
gnpc stop     # 停止
gnpc status   # 查看状态（进程/端口/通道/出口IP）
gnpc status --brief   # 一行摘要: hy2 120ms sel=auto-out auto=hy2-out frozen=no exit=8.209.203.17
gnpc tunnel   # 隧道诊断（兼容旧名 wg）
gnpc config --check  # 校验配置安全 + 与 config.toml 一致性
gnpc test     # 测试代理连通性
gnpc env      # CLI 代理环境状态（gnp-on/gnp-off 快捷开关）
gnpc peer gnp.cfg  # 从 server 发来的 gnp.cfg 一键接入
gnpc switch ssh    # 强制 TCP 兜底（urltest 会自动选路, auto 交还）
gnpc guard         # 手动跑一次看门狗（正常由 tick.sh 每分钟调）
gnpc rules-update  # 立即更新规则集（正常由 tick.sh 04 点窗口调）
gnpc migrate --dry-run   # 只看会改什么, 不落盘
```

### 3. 使用代理

#### macOS（系统代理）

```bash
gnpc proxy --on       # 开启系统代理 (osascript 弹授权, 不存密码)
gnpc proxy --status   # 查看代理状态
gnpc proxy --off      # 关闭系统代理
```

> 开启后 Safari / Chrome 等浏览器自动走 sing-box 代理。

#### Linux（环境变量 / GNOME）

```bash
# 方式 A: 环境变量（终端程序）
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080
export all_proxy=socks5://127.0.0.1:1080

# 方式 B: GNOME 系统代理
gnpc proxy --on

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
| Server 地址 | `8.209.203.17:5766`（UDP/QUIC；2026-10-05 从 443 迁出） |
| 认证方式 | Hysteria2 密码 + salamander obfs（两者都在 `config.toml`；由 `gnps gen-user` 生成 gnp.cfg） |
| 证书 | 自签证书 `/opt/gnp/certs/`（客户端 `insecure: true` 信任） |

> 密码需要安全传输，不影响 server 安全性。密码池存在 gitee 私有仓库。

## 部署矩阵（2026-10-05 全量迁到 v2 布局：`~/.local/gnp` / `/opt/gnp` + `config.toml`）

| 节点 | 角色 | 部署根（二进制+配置+规则+secrets） | 常驻服务 | 调度 | 实例 |
|---|---|---|---|---|---|
| Mac | client | `~/.local/gnp/` | launchd `com.gnpc.singbox` | launchd `com.gnpc.tick` (60s) | 单一 ✓ |
| aipro | client + 无线路由 | `/home/lwboy/.local/gnp/` | systemd `gnpc.service`（系统级） | root crontab 单行 tick | 宿主 1 个 ✓（路由容器内 sing-box 是独立角色, 共存不算残留） |
| lwmate | client | `/home/lwboy/.local/gnp/` | systemd `gnpc.service` | 用户 crontab 单行 tick | 单一 ✓ |
| cozepc (火山 x86_64) | client | `/root/.local/gnp/` | systemd `gnpc.service` | root crontab 单行 tick | ✅ 出口 8.209.203.17, github/google 200; sing-box 官方 release 自带 with_quic, 无需自编译 |
| lwtop | server | `/opt/gnp/` | systemd `gnps.service` | root crontab 单行 tick（`GNP_HOME=/opt/gnp`） | 单一 ✓ (UDP 5766) |
| vmwin / lwwin | client | 待迁移：`%~\.local\gnp` | 计划任务 `gnpc` (ONLOGON) | 暂不支持 (Windows 无 tick) | 离线中（plan D6），资产已备好，开机后 `gnpc migrate` |

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
├── Cargo.toml / crates/     # gnp 产品: gnp-core + gnp-client + gnp-server
├── config/safe-template.json# 安全配置模板 (mixed + hysteria2)
├── bash/                    # 构建安装脚本
├── docs/                    # 产品文档 (usage/architecture/auto-registration + finished-plans 归档)
├── vendor/sing-box/         # submodule: sing-box 源码
└── deploy/                  # 个人部署资产 (aipro-wifi / ns-hub / peers / ...) — 与产品分离
```

## 仓库

- main: github
- backup: gitee （双远端同步）

## License

MIT