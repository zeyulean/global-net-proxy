# global-net-proxy 使用手册

> **国内外网络分流工具**：国外流量（github / google / 代码源 / AI API）走 WireGuard 隧道经海外出口，国内流量直连。
>
> 客户端使用 sing-box **mixed 代理模式**（socks5+http 端口 1080），不碰路由表，零断网风险。

---

## 目录

- [快速开始](#快速开始)
- [gnp-client 命令详解](#gnp-client-命令详解)
  - [start — 启动代理](#start--启动代理)
  - [stop — 停止代理](#stop--停止代理)
  - [status — 查看状态](#status--查看状态)
  - [config — 查看/校验配置](#config--查看校验配置)
  - [wg — WireGuard 隧道诊断](#wg--wireguard-隧道诊断)
  - [test — 测试代理连通性](#test--测试代理连通性)
  - [install — 安装 sing-box + 生成配置](#install--安装-sing-box--生成配置)
  - [register — 自动注册新机器](#register--自动注册新机器)
  - [update-rules — 规则集更新 + 守护](#update-rules--规则集更新--守护)
  - [cleanup — 应急清理](#cleanup--应急清理)
  - [recover — 断网恢复](#recover--断网恢复)
  - [proxy — 系统代理开关](#proxy--系统代理开关)
- [gnp-server 命令详解](#gnp-server-命令详解)
  - [install — 安装 WireGuard server](#install--安装-wireguard-server)
  - [uninstall — 卸载 server](#uninstall--卸载-server)
  - [status — 查看状态](#status--查看状态-1)
  - [peers — 列出客户端](#peers--列出客户端)
  - [add-peer — 添加客户端](#add-peer--添加客户端)
  - [pregen — 预生成 peer 池](#pregen--预生成-peer-池)
  - [activate — 激活预生成的 peer](#activate--激活预生成的-peer)
- [典型场景](#典型场景)
  - [场景一：首次部署（从零开始）](#场景一首次部署从零开始)
  - [场景二：新机器加入](#场景二新机器加入)
  - [场景三：日常使用](#场景三日常使用)
  - [场景四：故障排查](#场景四故障排查)
- [macOS vs Linux 差异](#macos-vs-linux-差异)
- [技术细节](#技术细节)

---

## 快速开始

### 第 0 步：安装 gnp CLI

```bash
# 克隆项目 (含 submodule)
git clone --recurse-submodules <repo-url>
cd global-net-proxy

# 构建 + 安装到系统 PATH
bash bash/install.sh

# 验证
gnp-client --version   # gnp-client 0.1.0
gnp-server --version   # gnp-server 0.1.0
```

### 第 1 步：部署 Server（海外节点）

```bash
sudo gnp-server install            # 安装 WireGuard + NAT + 开机自启
sudo gnp-server add-peer macbook   # 为每台客户端机器生成配置
sudo gnp-server pregen 20          # 预生成 20 个 peer 备用
```

### 第 2 步：部署 Client（本机）

```bash
# 方式 A: 自动注册（推荐）
export GITEE_TOKEN=xxxx
gnp-client register my-client-id
# 然后在 server 上: sudo gnp-server activate my-client-id

# 方式 B: 手动安装
gnp-client install \
  --server 8.209.203.17 \
  --server-pubkey <pubkey> \
  --client-privkey <privkey> \
  --client-ip 10.0.0.5/32

# 启动代理
gnp-client start
```

### 第 3 步：设置代理

#### macOS

```bash
# 开启系统代理（Safari/Chrome 等自动走代理，会弹管理员授权窗口）
gnp-client proxy --on

# 关闭
gnp-client proxy --off

# 查看状态
gnp-client proxy --status
```

#### Linux

```bash
# 方式 A: 环境变量（终端程序）
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080
export all_proxy=socks5://127.0.0.1:1080

# 方式 B: GNOME 系统代理
gnp-client proxy --on    # 通过 gsettings 设置

# 方式 C: 单次使用
curl -x socks5h://127.0.0.1:1080 https://www.google.com
```

### 第 4 步：验证

```bash
gnp-client status    # 查看状态
gnp-client test      # 测试代理连通性
gnp-client wg        # WireGuard 隧道诊断
```

---

## gnp-client 命令详解

gnp-client 管理本机 sing-box mixed 代理（wg 隧道），共 **12 个子命令**。

> **安全原则**：本工具只使用 mixed 代理模式（socks5+http on 127.0.0.1:1080），不修改系统路由表，零断网风险。绝不用 tun 模式。
>
> **数据目录**：`~/.local/share/sing-box/`
> - 二进制：`sing-box`
> - 配置：`config.json`
> - 规则集：`rules/*.srs`

---

### start — 启动代理

启动 sing-box 代理服务，并注册为开机自启。

```bash
gnp-client start
```

**行为说明**：

| 平台 | 服务管理器 | 开机自启文件 |
|------|-----------|-------------|
| macOS | launchctl | `~/Library/LaunchAgents/com.gnp.sing-box.plist` |
| Linux | systemd | `/etc/systemd/system/gnp-proxy.service` |

- macOS：写入/加载 launchd plist，KeepAlive=true 崩溃自动重启
- Linux：如未安装 systemd 单元会自动创建，然后 `systemctl start gnp-proxy`
- 启动后代理监听 `0.0.0.0:1080`（socks5+http）

**示例**：

```bash
gnp-client start
# ♻️  启动 sing-box (macos)...
# ✅ sing-box 已启动 (socks5+http on 127.0.0.1:1080)
```

---

### stop — 停止代理

停止 sing-box 代理服务，卸载开机自启并杀掉残留进程。

```bash
gnp-client stop
```

**行为说明**：

- macOS：`launchctl unload` plist + `pkill -f "sing-box run"`
- Linux：`systemctl stop gnp-proxy`

---

### status — 查看状态

显示完整的代理运行状态。

```bash
gnp-client status
```

**输出内容**：

1. **安装状态**：sing-box 二进制是否存在、config.json 是否存在
2. **进程状态**：是否运行中
3. **端口**：1080 是否在监听
4. **配置安全检查**：
   - 无 tun/strict_route（✅ 安全）
   - 有 mixed inbound
   - 有 wg endpoint
5. **隧道出口**：如果运行中，检测出口 IP 和延迟

**示例输出**：

```
== gnp-client 状态 (macos) ==

📦 安装:
  sing-box 二进制: 已安装 ✅
  配置文件: 存在 ✅

🔄 进程:
  运行状态: 运行中 ✅
  端口 1080: 监听中 ✅

🔒 配置安全:
  无 tun/strict_route: ✅
  mixed inbound: ✅
  wg endpoint: ✅

🌐 wg 隧道:
  出口 IP: 8.209.203.17 (234ms)
```

---

### config — 查看/校验配置

查看 sing-box 配置文件内容，或校验配置是否安全。

```bash
# 校验配置安全性
gnp-client config --check

# 显示完整配置内容 (JSON)
gnp-client config --show
```

**校验项**：

| 检查项 | 说明 |
|--------|------|
| 无 tun/strict_route | 确保不包含 `strict_route` 或 `auto_route`（危险！） |
| mixed inbound | 确保有 mixed 类型入站（socks5+http） |
| wg endpoint | 确保有 WireGuard outbound |

如果检测到危险配置（tun/strict_route），会以非零退出码报错。

---

### wg — WireGuard 隧道诊断

显示隧道配置详情并测试连通性。

```bash
gnp-client wg
```

**输出内容**：

1. **隧道配置**（从 config.json 提取）：
   - 本机 wg IP（如 `10.0.0.5/32`）
   - 远端 server 地址和端口
   - MTU（默认 1280）
   - 密钥状态
2. **隧道连通性**：通过 socks5h 代理检测出口 IP
3. **代理测试**：测试 github、google 是否可达

**示例输出**：

```
== wg 隧道诊断 (macos) ==

📋 隧道配置:
  本机 wg IP: 10.0.0.5/32
  远端 server: 8.209.203.17:1194
  MTU: 1280
  密钥: 已配置 (44 字符)

🌐 隧道连通性:
  ✅ 出口 IP: 8.209.203.17 (234ms)

🔍 代理测试:
  github: HTTP 200 (156ms)
  google: HTTP 200 (89ms)
```

---

### test — 测试代理连通性

快速测试代理是否可用。

```bash
gnp-client test
```

**测试内容**：

1. 通过 `socks5h://127.0.0.1:1080` 检测出口 IP
2. 测试 github（`https://api.github.com/zen`）
3. 测试 google（`https://www.google.com`）

> 使用 `socks5h`（带 h）表示 DNS 在代理端远程解析，避免本地 DNS 污染。

---

### install — 安装 sing-box + 生成配置

下载 sing-box 二进制、下载规则集、生成 mixed+wg 配置。

```bash
gnp-client install \
  --server <SERVER_IP> \
  --server-pubkey <SERVER_PUBKEY> \
  --client-privkey <CLIENT_PRIVKEY> \
  --client-ip <CLIENT_IP/32> \
  [--wg-port 1194] \
  [--bin-only]
```

**参数说明**：

| 参数 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `--server` | ✅ | — | 远端 wg server 地址（IP 或域名） |
| `--server-pubkey` | ✅ | — | 远端 server 公钥 |
| `--client-privkey` | ✅ | — | 本机 WireGuard 私钥 |
| `--client-ip` | ✅ | — | 本机 wg IP（如 `10.0.0.5/32`） |
| `--wg-port` | ❌ | 1194 | wg 端口 |
| `--bin-only` | ❌ | false | 只下载 sing-box，不生成配置 |

**行为说明**：

1. 如果 sing-box 未安装，自动下载 sing-box **v1.12.3**（⚠️ 非 1.13，endpoint wg 有 bug）
2. 下载规则集（geosite-cn、geoip-cn、google、github、openai 等）
3. 生成 `config.json`（mixed + wg outbound 格式）
4. Linux 上自动安装 systemd 系统级服务（开机自启）

**示例**：

```bash
gnp-client install \
  --server 8.209.203.17 \
  --server-pubkey M/t3YYwIW7Xou+vASGtNoAHHNrh82ROzYDU4LIsLz18= \
  --client-privkey <你的私钥> \
  --client-ip 10.0.0.5/32
```

> ⚠️ 私钥需要安全传输，不要泄露。

---

### register — 自动注册新机器

从 gitee 私有仓库的 peer 池自动取配置，一键完成安装。

```bash
# 设置 token
export GITEE_TOKEN=xxxx

# 自动注册（client_id 默认用 hostname）
gnp-client register

# 指定 client_id
gnp-client register --client-id macbook

# 只查看 peer 池状态，不修改
gnp-client register --list

# 试运行（看会选中哪个 peer，不实际修改）
gnp-client register --dry-run
```

**参数说明**：

| 参数 | 说明 |
|------|------|
| `--client-id <ID>` | 客户端标识（可选，默认用 hostname） |
| `--list` | 列出 peer 池状态（available / used / activated） |
| `--dry-run` | 只看会选中哪个 peer，不实际修改 |

**工作流程**：

1. 克隆 gitee 私有仓库（需要 `GITEE_TOKEN`）
2. 读取 `peers/` 目录下的 peer JSON
3. 选择一个 `status=available` 的 peer（优先匹配 client_id）
4. 标记为 `used` 并 push 回 gitee
5. 校验 server 公钥（防篡改）
6. 生成 sing-box config.json
7. 下载安装 sing-box + 规则集
8. 安装 systemd 服务（Linux）
9. 验证配置

> ⚠️ **重要**：register 完成后，需要在 **server** 上执行 `gnp-server activate <client_id>` 来将 peer 加入 wg0 运行时。

---

### update-rules — 规则集更新 + 守护

更新 sing-box 规则集（geosite/geoip），或检查守护进程。

```bash
# 检查 sing-box 是否运行，挂了就重启（默认行为）
gnp-client update-rules
# 等价于
gnp-client update-rules --check

# 强制更新规则集（重启 sing-box 加载最新 remote rule-set）
gnp-client update-rules --update

# 安装 cron 任务（每天 04:00 自动检查）
gnp-client update-rules --install-cron
```

**参数说明**：

| 参数 | 说明 |
|------|------|
| `--update` | 强制重启 sing-box，触发 remote rule-set 重新拉取 |
| `--check` | 检查 sing-box 是否运行，挂了就重启（默认行为） |
| `--install-cron` | 安装 crontab 任务，每天 04:00 执行 `update-rules --check` |

**cron 说明**：

安装后会添加一条 crontab：

```
0 4 * * * /path/to/gnp-client update-rules check >> ~/.local/share/sing-box/cron.log 2>&1
```

---

### cleanup — 应急清理

彻底清理 sing-box 所有残留。参考 [aipro 断网事故](incident-2026-08-10.md)。

```bash
gnp-client cleanup
```

**清理步骤（6 步）**：

1. **停止服务**：systemctl stop / launchctl unload / pkill -9 sing-box
2. **禁用开机自启**：systemctl disable / mask
3. **清理 tun 接口**：删除 gnp0、tun0
4. **清理策略路由**：删除 priority 9000-9010 的 ip rule，flush table 2022
5. **恢复默认路由**：探测网关并恢复
6. **备份数据目录**：`~/.local/share/sing-box/` → `sing-box.disabled-<timestamp>`

> ⚠️ 这条命令会**彻底清除** sing-box，之后需要重新 `gnp-client install` 或 `register`。

---

### recover — 断网恢复

sing-box tun 模式破坏路由表后的网络恢复工具。

```bash
gnp-client recover
```

**恢复步骤（5 步）**：

1. **停止 sing-box 服务**（破坏路由的元凶）
2. **清理策略路由**：`ip rule flush`
3. **清理独立路由表**：flush table 2022/100/200
4. **恢复默认路由**：尝试常见网关（192.168.0.1 / 192.168.1.1 / 10.0.0.1）
5. **清理 tun 接口**：删除 gnp0、tun0
6. **恢复 DNS**：写入 `223.5.5.5` + `119.29.29.29` 到 `/etc/resolv.conf`

> 💡 如果 recover 后仍不通，直接 `reboot` 重启机器。

---

### proxy — 系统代理开关

设置或取消操作系统层面的代理，让浏览器等 GUI 程序走 sing-box。

```bash
# 查看当前状态
gnp-client proxy --status

# 开启系统代理
gnp-client proxy --on

# 关闭系统代理
gnp-client proxy --off

# 无参数：显示状态和用法
gnp-client proxy
```

**参数说明**：

| 参数 | 说明 |
|------|------|
| `--on` | 开启系统代理 |
| `--off` | 关闭系统代理 |
| `--status` | 查看当前代理状态 |

**平台差异**：

| 平台 | 实现方式 | 说明 |
|------|---------|------|
| macOS | `networksetup` + `osascript` | 设置 HTTP/HTTPS/SOCKS 代理，弹管理员授权窗口（不存密码） |
| Linux (GNOME) | `gsettings` | 设置 `org.gnome.system.proxy` 为 manual 模式 |
| Linux (无 GNOME) | 提示 export | 输出环境变量设置命令 |

**macOS 注意事项**：

- 通过 `osascript` 弹出系统管理员授权窗口，需要用户点击允许
- **不存储密码**，每次 --on 都会弹窗
- 会自动检测活跃网络服务（Wi-Fi / Ethernet）

---

## gnp-server 命令详解

gnp-server 管理 WireGuard server（内核 wg0），共 **7 个子命令**。

> **所有命令需要 root 权限**（`wg set` / `iptables`），请用 `sudo` 运行。
>
> **配置文件**：`/etc/wireguard/wg0.conf`
> **端口**：1194
> **网段**：`10.0.0.0/24`

---

### install — 安装 WireGuard server

安装 WireGuard server，包括软件包、密钥、配置、NAT、开机自启。

```bash
sudo gnp-server install
```

**安装步骤**：

1. **安装 wireguard**：`apt install wireguard wireguard-tools`
2. **生成 server 密钥**：`wg genkey` → `wg pubkey`
3. **写 wg0.conf**：`/etc/wireguard/wg0.conf`（Address=10.0.0.1/24, ListenPort=1194）
4. **启动 wg-quick**：`systemctl enable --now wg-quick@wg0`
5. **配置 NAT**：
   - 自动检测出口网卡（`ip route show default`）
   - `iptables -t nat -A POSTROUTING -s 10.0.0.0/24 -j MASQUERADE`
   - `iptables -A FORWARD -i wg0 -j ACCEPT`
   - 开启 `net.ipv4.ip_forward=1`

**安装完成后**：

```
✅ Server 安装完成!
  server IP: 10.0.0.1/24
  端口: 1194
  公钥: M/t3YYwIW7Xou+vASGtNoAHHNrh82ROzYDU4LIsLz18=
  出口网卡: eth0

下一步: gnp-server add-peer <名称> 添加客户端
```

> ⚠️ 阿里云安全组需要放行 **UDP 1194** 端口。

---

### uninstall — 卸载 server

完全卸载 WireGuard server。

```bash
sudo gnp-server uninstall
```

**卸载内容**：

1. 停止并禁用 `wg-quick@wg0`
2. 删除 `/etc/wireguard/wg0.conf`
3. 删除 server 密钥（server.key / server.pub）
4. 删除 pending-peers 目录
5. 移除 wireguard-tools 包

---

### status — 查看状态

查看 wg0 接口状态和 peer 信息。

```bash
sudo gnp-server status
```

**输出内容**：

- wg0 是否激活
- `wg show wg0` 原始输出（接口详情、所有 peer 的握手时间/传输量）

---

### peers — 列出客户端

列出所有已注册的客户端 peer。

```bash
sudo gnp-server peers
```

**输出**：所有 peer 的公钥列表。

---

### add-peer — 添加客户端

为新客户端生成密钥、分配 IP 并加入 wg0。

```bash
sudo gnp-server add-peer <名称>
```

**参数**：

| 参数 | 说明 |
|------|------|
| `name` | 客户端名称（如 macbook、aipro、win-01） |

**行为说明**：

1. 生成客户端密钥对（`wg genkey` → `wg pubkey`）
2. 分配客户端 IP（10.0.0.x）
3. 将 peer 加入 wg0 运行时（`wg set wg0 peer ... allowed-ips ...`）
4. 持久化到 wg0.conf
5. 输出客户端配置文件内容

**输出示例**：

```
== 添加客户端: macbook ==
  client IP: 10.0.0.2

✅ 客户端已添加!
==================
[Interface]
PrivateKey = <客户端私钥>
Address = 10.0.0.2/32

[Peer]
PublicKey = M/t3YYwIW7Xou+vASGtNoAHHNrh82ROzYDU4LIsLz18=
Endpoint = <SERVER_IP>:1194
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
==================
```

> ⚠️ 输出的客户端私钥需要安全传输到客户端机器，不要泄露。

---

### pregen — 预生成 peer 池

批量生成待用 peer 配置包，不占运行时资源。

```bash
sudo gnp-server pregen <数量>
```

**参数**：

| 参数 | 说明 |
|------|------|
| `count` | 要预生成的 peer 数量 |

**行为说明**：

- 为每个 peer 生成密钥对和 IP
- 存为 JSON 到 `/etc/wireguard/pending-peers/<id>.json`
- JSON 包含：id、status=available、密钥、IP、server 信息
- 文件权限 600

**用途**：配合 `gnp-client register` 实现新机器自动注册。peer 池可推送到 gitee 私有仓库。

---

### activate — 激活预生成的 peer

将 pending-peers 中的 peer 加入 wg0 运行时。

```bash
sudo gnp-server activate <client_id>
```

**参数**：

| 参数 | 说明 |
|------|------|
| `id` | peer 的 client_id（pregen 时生成的 ID） |

**行为说明**：

1. 从 `/etc/wireguard/pending-peers/<id>.json` 读取配置
2. 将 peer 加入 wg0 运行时（`wg set`）
3. 持久化到 wg0.conf
4. 更新 JSON 状态为 `activated`

> ⚠️ `gnp-client register` 完成后，**必须**在 server 上执行此命令，否则客户端无法连通。

---

## 典型场景

### 场景一：首次部署（从零开始）

**在 Server 上（海外节点）**：

```bash
# 1. 安装 gnp CLI
bash bash/install.sh

# 2. 安装 WireGuard server
sudo gnp-server install

# 3. 预生成 peer 池（推荐）
sudo gnp-server pregen 20

# 4. 将 peers/ 目录推送到 gitee 私有仓库
cd /etc/wireguard/pending-peers/
# 复制到项目仓库的 peers/ 目录，push 到 gitee
```

**在 Client 上（每台机器）**：

```bash
# 1. 安装 gnp CLI
bash bash/install.sh

# 2. 自动注册
export GITEE_TOKEN=xxxx
gnp-client register my-machine

# 3. 回到 Server 上激活
sudo gnp-server activate my-machine

# 4. 启动代理
gnp-client start

# 5. 设置系统代理
# macOS:
gnp-client proxy --on
# Linux:
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080

# 6. 验证
gnp-client test
```

### 场景二：新机器加入

```bash
# 在新机器上
export GITEE_TOKEN=xxxx
gnp-client register new-machine-id
# → 自动取 peer、生成配置、安装 sing-box

# 在 Server 上激活
sudo gnp-server activate new-machine-id

# 启动
gnp-client start
```

### 场景三：日常使用

```bash
# 每天开机后代理已自动启动（systemd/launchd 开机自启）
# 只需设置代理：

# macOS（浏览器）
gnp-client proxy --on

# Linux（终端）
export http_proxy=http://127.0.0.1:1080 https_proxy=http://127.0.0.1:1080

# 查看状态
gnp-client status

# 测试连通性
gnp-client test
```

### 场景四：故障排查

```bash
# 1. 查看完整状态
gnp-client status

# 2. 检查配置安全
gnp-client config --check

# 3. 隧道诊断
gnp-client wg

# 4. 如果代理不工作，尝试重启
gnp-client stop
gnp-client start

# 5. 如果断网了（tun 模式残留）
gnp-client recover    # 恢复网络
gnp-client cleanup    # 彻底清理 sing-box
# 然后重新安装
gnp-client install ...
gnp-client start
```

---

## macOS vs Linux 差异

### 服务管理

| 项目 | macOS | Linux |
|------|-------|-------|
| 服务管理器 | launchctl | systemd |
| 服务标签 | `com.gnp.sing-box` | `gnp-proxy` |
| plist/unit 路径 | `~/Library/LaunchAgents/com.gnp.sing-box.plist` | `/etc/systemd/system/gnp-proxy.service` |
| 开机自启 | RunAtLoad=true, KeepAlive=true | WantedBy=multi-user.target |
| 崩溃重启 | KeepAlive=true | Restart=on-failure, RestartSec=10 |
| 查看服务状态 | `launchctl list \| grep gnp` | `systemctl status gnp-proxy` |

### 代理设置

| 项目 | macOS | Linux |
|------|-------|-------|
| 系统代理 | `gnp-client proxy --on`（networksetup + osascript） | `gnp-client proxy --on`（gsettings）或 export |
| 授权方式 | osascript 弹窗（不存密码） | 无需授权（gsettings 或环境变量） |
| 浏览器生效 | Safari/Chrome 自动走系统代理 | GNOME 应用走 gsettings；终端需 export |
| 终端代理 | 需手动 export | `export http_proxy=http://127.0.0.1:1080` |

### 数据目录（跨平台一致）

```
~/.local/share/sing-box/
├── sing-box          # 二进制
├── config.json       # 配置
├── rules/            # 规则集
│   ├── geosite-cn.srs
│   ├── geoip-cn.srs
│   └── ...
└── cron.log          # cron 日志
```

---

## 技术细节

### sing-box 版本

使用 **sing-box v1.12.3**。

> ⚠️ **不用 1.13**：sing-box 1.13 的 endpoint wireguard 有 bug（握手不完成），降级到 1.12.3 使用 outbound wireguard 格式。

### WireGuard outbound 格式

使用 **outbound wireguard**（1.12 格式），不是 endpoint（1.13 格式）。

需要环境变量：

```bash
ENABLE_DEPRECATED_WIREGUARD_OUTBOUND=true
```

- macOS launchd plist 中已内置此环境变量
- Linux systemd unit 中已内置此环境变量

配置片段：

```json
{
  "outbounds": [{
    "type": "wireguard",
    "tag": "wg-out",
    "local_address": ["10.0.0.5/32"],
    "private_key": "<本机私钥>",
    "peer_public_key": "<server公钥>",
    "server": "8.209.203.17",
    "server_port": 1194,
    "mtu": 1280,
    "system": false,
    "reserved": [0, 0, 0]
  }]
}
```

- `system: false` → userspace WireGuard，不需要内核模块，不需要 root

### 端口 1194

使用 **UDP 1194**，不是默认的 51820。

> 原因：阿里云安全组限制，1194 是允许的端口。

需要在云服务商安全组放行 **UDP 1194**。

### 代理模式：mixed（绝不 tun）

| 对比项 | tun 模式（❌ 危险） | mixed 模式（✅ 安全） |
|--------|-------------------|---------------------|
| 路由表 | `strict_route` + `auto_route` 接管 | **完全不碰** |
| 权限 | 需要 root | **普通用户即可** |
| 断网风险 | 高（路由被接管后 SSH 不通） | **零**（只开代理端口） |
| 透明代理 | 是（系统级） | 否（需设置 http_proxy） |

mixed 模式监听 `0.0.0.0:1080`，同时支持 socks5 和 http 代理协议。

### DNS 分流

| 域名类型 | DNS 服务器 | 路径 |
|---------|-----------|------|
| 国内域名（geosite-cn） | `223.5.5.5` | 直连（detour=direct） |
| 国外域名 | `1.1.1.1` | 经 wg 隧道（detour=wg-out） |

- 国外域名 DNS 经 wg 隧道走 1.1.1.1 **UDP**（非 DoH），避免 DNS 污染
- 使用 `socks5h`（带 h）进行 HTTP 代理时，DNS 在代理端远程解析

### 路由规则

```json
{
  "route": {
    "rules": [
      { "rule_set": ["geosite-cn", "geoip-cn"], "outbound": "direct" },
      { "ip_is_private": true, "outbound": "direct" }
    ],
    "final": "wg-out"
  }
}
```

| 流量类型 | 判定 | 出口 |
|---------|------|------|
| 国内域名/IP | geosite-cn / geoip-cn | direct（直连） |
| 私有 IP | ip_is_private | direct（直连） |
| 其余所有 | final | wg-out（走隧道） |

### Server 信息

| 项目 | 值 |
|------|------|
| Server 地址 | `8.209.203.17:1194` |
| Server 公钥 | `M/t3YYwIW7Xou+vASGtNoAHHNrh82ROzYDU4LIsLz18=` |
| Client 网段 | `10.0.0.2` – `10.0.0.250` |

> Server 公钥可以公开，不影响安全性。Client 私钥存在 gitee 私有仓库。

---

> 📖 相关文档：[架构设计](architecture.md) | [安装指南](setup.md) | [自动注册](auto-registration.md) | [断网事故记录](incident-2026-08-10.md)
