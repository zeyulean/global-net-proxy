# global-net-proxy 使用手册

> **国内外网络分流工具**：国外流量（github / google / 代码源 / AI API）走 Hysteria2 (QUIC) 隧道经海外出口，国内流量直连。
>
> 客户端使用 sing-box **mixed 代理模式**（socks5+http 端口 1080），不碰路由表，零断网风险。

---

## 目录

- [快速开始](#快速开始)
- [gnpc 命令详解](#gnpc-命令详解)
  - [start — 启动代理](#start--启动代理)
  - [stop — 停止代理](#stop--停止代理)
  - [status — 查看状态](#status--查看状态)
  - [config — 查看/校验配置](#config--查看校验配置)
  - [wg — Hysteria2 隧道诊断](#wg--hysteria2-隧道诊断)
  - [test — 测试代理连通性](#test--测试代理连通性)
  - [init — 生成 config.toml](#init--生成-configtoml)
  - [install — 按 config.toml 装齐](#install--按-configtoml-装齐)
  - [migrate — 旧布局一键迁移](#migrate--旧布局一键迁移)
  - [install-scheduler — 装/卸调度](#install-scheduler--装载调度)
  - [双通道架构与自动降级](#双通道架构与自动降级2026-10-05-新增)
  - [probe — 主动诊断](#probe--主动诊断hy2-握手--mtu-扫描)
  - [switch — 手动通道切换](#switch--手动通道切换)
  - [guard — 通道看门狗](#guard--通道看门狗退避探测--故障冻结--告警)
  - [register — 自动注册新机器](#register--自动注册新机器)
  - [rules-update — 规则集日更](#rules-update--规则集日更)
  - [cleanup — 应急清理](#cleanup--应急清理)
  - [recover — 断网恢复](#recover--断网恢复)
  - [proxy — 系统代理开关](#proxy--系统代理开关)
- [gnps 命令详解](#gnps-命令详解)
  - [install — 安装 Hysteria2 server](#install--安装-hysteria2-server)
  - [uninstall — 卸载 server](#uninstall--卸载-server)
  - [status — 查看状态](#status--查看状态-1)
  - [users — 列出用户](#users--列出用户)
  - [add-user — 添加用户](#add-user--添加用户)
  - [pregen — 预生成用户密码池](#pregen--预生成用户密码池)
  - [activate — 激活预生成的用户](#activate--激活预生成的用户)
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
gnpc --version   # gnpc 0.1.0
gnps --version   # gnps 0.1.0
```

### 第 1 步：部署 Server（海外节点）

```bash
# 唯一事实源: deploy/hosts/lwtop-server.toml → /opt/gnp/config.toml
# (8 个用户 / UDP 5766 / salamander obfs 都在这份 toml 里)
scp deploy/hosts/lwtop-server.toml lwtop:/tmp/
ssh lwtop 'sudo install -m 600 /tmp/lwtop-server.toml /opt/gnp/config.toml
           sudo /opt/gnp/bin/gnps install --config /opt/gnp/config.toml'
# gnps install = 渲染 config.json + sing-box check + gnps.service + tick 调度 + 开机自启

sudo /opt/gnp/bin/gnps gen-user --name macbook  # 加用户 + 出 gnp.cfg (含 5766 + obfs)
sudo /opt/gnp/bin/gnps pregen 20                 # 预生成 20 个用户密码备用
# ⚠️ hy2 端口改动必须 ufw + 云安全组双侧, 且 ufw 持久化 (禁裸 iptables)
```

### 第 2 步：部署 Client（本机）

```bash
# 方式 A: 自动注册（推荐）
export GITEE_TOKEN=xxxx
gnpc register my-client-id
# 然后在 server 上: sudo gnps activate my-client-id

# 方式 B: 手动安装 (推荐: 配置全在 config.toml, 唯一事实源)
gnpc init \
  --server 8.209.203.17 \
  --hy2-password <HY2_PASSWORD> \
  --obfs-password <OBFS_PASSWORD> \
  --listen 127.0.0.1        # mac 单机自用; 局域网服务机填 0.0.0.0
gnpc install                 # 下载 sing-box + 规则集, 渲染 config.json, 装常驻服务
gnpc install-scheduler       # 装调度 (tick.sh + launchd/crontab), 并清旧
gnpc start

# 方式 C: 从旧布局迁移 (存量机器)
scp deploy/hosts/<host>.toml <host>:~/.local/gnp/config.toml
scp gnpc <host>:/tmp/ && ssh <host> '/tmp/gnpc migrate --config ~/.local/gnp/config.toml'
```

### 第 3 步：设置代理

#### macOS

```bash
# 开启系统代理（Safari/Chrome 等自动走代理，会弹管理员授权窗口）
gnpc proxy --on

# 关闭
gnpc proxy --off

# 查看状态
gnpc proxy --status
```

#### Linux

```bash
# 方式 A: 环境变量（终端程序）
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080
export all_proxy=socks5://127.0.0.1:1080

# 方式 B: GNOME 系统代理
gnpc proxy --on    # 通过 gsettings 设置

# 方式 C: 单次使用
curl -x socks5h://127.0.0.1:1080 https://www.google.com
```

### 第 4 步：验证

```bash
gnpc status    # 查看状态
gnpc test      # 测试代理连通性
gnpc tunnel    # Hysteria2 隧道诊断 (兼容旧名 wg)
```

---

## gnpc 命令详解

gnpc 管理本机 sing-box mixed 代理（hysteria2/QUIC 隧道），共 **12 个子命令**。

> **安全原则**：本工具只使用 mixed 代理模式（socks5+http on 127.0.0.1:1080），不修改系统路由表，零断网风险。绝不用 tun 模式。
>
> **部署根**：`~/.local/gnp/`（`$GNP_HOME` 可覆盖）
> ```
> bin/{gnpc,sing-box,tick.sh}   config.toml ← 唯一事实源   config.json ← 生成物
> etc/tick.d/*.sh   rules/*.srs   secrets/ (0600)   var/ (日志/状态/cache.db)   backups/
> ```
> **`config.toml` 是唯一事实源**：`config.json` / 服务单元 / `tick.sh` / 容器 secrets
> 全是生成物，**勿手改**。改配置 → 改 toml → `gnpc install` → 重启服务。

---

### start — 启动代理

启动 sing-box 代理服务，并注册为开机自启。

```bash
gnpc start
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
gnpc start
# ♻️  启动 sing-box (macos)...
# ✅ sing-box 已启动 (socks5+http on 127.0.0.1:1080)
```

---

### stop — 停止代理

停止 sing-box 代理服务，卸载开机自启并杀掉残留进程。

```bash
gnpc stop
```

**行为说明**：

- macOS：`launchctl unload` plist + `pkill -f "sing-box run"`
- Linux：`systemctl stop gnp-proxy`

---

### status — 查看状态

显示完整的代理运行状态。

```bash
gnpc status
```

**输出内容**：

1. **安装状态**：sing-box 二进制是否存在、config.json 是否存在
2. **进程状态**：是否运行中
3. **端口**：1080 是否在监听
4. **配置安全检查**：
   - 无 tun/strict_route（✅ 安全）
   - 有 mixed inbound
   - 有 hysteria2 outbound
5. **隧道出口**：如果运行中，检测出口 IP 和延迟

**示例输出**：

```
== gnpc 状态 (macos) ==

📦 安装:
  sing-box 二进制: 已安装 ✅
  配置文件: 存在 ✅

🔄 进程:
  运行状态: 运行中 ✅
  端口 1080: 监听中 ✅

🔒 配置安全:
  无 tun/strict_route: ✅
  mixed inbound: ✅
  hysteria2 outbound: ✅

🌐 隧道:
  出口 IP: 8.209.203.17 (234ms)
```

---

### config — 查看/校验配置

查看 sing-box 配置文件内容，或校验配置是否安全。

```bash
# 校验配置安全性
gnpc config --check

# 显示完整配置内容 (JSON)
gnpc config --show
```

**校验项**：

| 检查项 | 说明 |
|--------|------|
| 无 tun/strict_route | 确保不包含 `strict_route` 或 `auto_route`（危险！） |
| mixed inbound | 确保有 mixed 类型入站（socks5+http） |
| hysteria2 outbound | 确保有 hysteria2 outbound |

如果检测到危险配置（tun/strict_route），会以非零退出码报错。

---

### tunnel — Hysteria2 隧道诊断

显示隧道配置详情并测试连通性。
（2026-08-15 起主命令为 `tunnel`，旧名 `wg` 保留为兼容别名——wg 时代历史沿用。）

```bash
gnpc tunnel   # 或旧名 gnpc tunnel
```

### env — CLI 代理环境变量开关（2026-08-15 新增）

CLI 工具（curl/pip/uv/npm/git 等）**不读系统代理**，只认环境变量。本命令输出 eval 装载：

```bash
eval "$(gnpc env --on)"    # 当前 shell 立即生效
eval "$(gnpc env --off)"   # 取消
eval "$(gnpc env --hook)"  # 写入 ~/.zshrc → 得到 gnp-on / gnp-off 快捷函数
gnpc env                   # 查看当前 shell 状态
```

- 直接运行 `env --on`（不经 eval）只打印不生效——终端下会输出引导提示
- 国内流量无需 no_proxy 白名单：sing-box 按 geosite-cn 内部分流直连
- Windows PowerShell 等价：`$env:https_proxy='http://127.0.0.1:1080'; $env:http_proxy=$env:https_proxy`

**输出内容**：

1. **隧道配置**（从 config.json 提取）：
   - 远端 server 地址和端口（`8.209.203.17:443`）
   - 密码状态（已配置，长度）
2. **隧道连通性**：通过 socks5h 代理检测出口 IP
3. **代理测试**：测试 github、google 是否可达

**示例输出**：

```
== 隧道诊断 (macos) ==

📋 隧道配置:
  远端 server: 8.209.203.17:443 (UDP/QUIC)
  密码: 已配置 (16 字符)

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
gnpc test
```

**测试内容**：

1. 通过 `socks5h://127.0.0.1:1080` 检测出口 IP
2. 测试 github（`https://api.github.com/zen`）
3. 测试 google（`https://www.google.com`）

> 使用 `socks5h`（带 h）表示 DNS 在代理端远程解析，避免本地 DNS 污染。

---

### init — 生成 config.toml（唯一事实源）

交互式或纯 flag 生成 `~/.local/gnp/config.toml`。**这是配置的唯一入口**：
`config.json` / 服务单元 / `tick.sh` / 容器 secrets 全由它渲染，勿手改生成物。

```bash
gnpc init \
  [--server <IP>] [--hy2-password <密码>] [--obfs-password <obfs密码>] \
  [--hy2-port 5766] [--listen 127.0.0.1] \
  [--hosts 'aipro.host=192.168.1.2,lwtop.host=8.209.203.17'] \
  [--out <路径>] [--force]
```

**要点**：

- 给了 `--server` + `--hy2-password` 就是非交互（适合 scp 到目标机直接落盘）；
  缺项会从旧 `config.json` 推断，或进交互提问
- 已存在的 `config.toml` **不覆盖**（要重写加 `--force`）
- 缺省端口 `hy2_port = 5766`（`GNP_PORT`，代码默认值）
- `ssh_key` 支持 `~`（由 gnpc 展开）；`ssh_user` 置空 = 关闭 ssh 兜底；
  `clash_api_port = 0` = 关闭 clash_api
- `[client.hosts]` 的域名含点，**key 必须加引号**：`"aipro.host" = "1.2.3.4"`
  （不加引号 TOML 会当成嵌套表）

### install — 按 config.toml 装齐

```bash
gnpc install [--config <config.toml 路径>] [--bin-only]
```

事实源优先级：`--config` > `$GNP_HOME/config.toml`。

**行为**：

1. sing-box 未安装则下载 **v1.13.16**（直连不通时自动借本地代理 `127.0.0.1:1080`）
2. 下载规则集（geosite-cn、geoip-cn、google、github、openai、anthropic、docker）
3. **渲染 `config.json`**：双通道形态（selector → urltest → hy2 + ssh 兜底），
   `cache_file` 落 `var/cache.db`（绝对路径，systemd CWD=/ 不可写）
4. **`sing-box check` 校验生成物**——不过就不算装成功
5. 写 `secrets/hy2-password` + `secrets/hy2-obfs`（0600 **带换行**；容器挂载源）
6. Linux 写 systemd `gnpc.service`（需 root；有免密 sudo 就借，没有则打印精确命令）

> 调度（`tick.sh` + launchd/crontab）由 `gnpc install-scheduler` 负责——
> 两者生命周期不同，故意不合并。

### migrate — 旧布局一键迁移

把 v1 的 `~/.local/share/sing-box` 迁到 v2 的 `~/.local/gnp`，一条命令完成。

```bash
gnpc migrate [--config deploy/hosts/<host>.toml] [--dry-run]
```

**事实源优先级**：`--config` > 已有 `$GNP_HOME/config.toml` > 解析旧 `config.json`。
> 用已有/`--config` 给的 toml 时是**逐字拷贝**（不重新序列化），
> 这样各机 `config.toml` 与 `deploy/hosts/<host>.toml` 的 md5 一致（可交叉校验）。

**顺序是硬要求**（同机同端口，旧的不让位新的必 crash-loop）：

```
落盘 (tick.sh/plist/unit) → 停旧常驻(保留单元可回滚) → 装载新
  → 三重验证(进程在跑 + 1080 在听 + 出口 IP 真是服务端)
  → 失败: 自动把旧单元装回去并中止
  → 通过: 才清旧残留 + 旧目录挪 backups/legacy-singbox/
```

**幂等**：可重复跑。`--dry-run` 只报告将要做什么，不落盘不动服务。

### install-scheduler — 装/卸调度

调度唯一入口 = `tick.sh`（全部机器同一份，编进二进制；改 `deploy/scheduler/tick.sh`
需重编）。每分钟一次：Mac = launchd `com.gnpc.tick`（StartInterval=60），
Linux = crontab **一行**（`GNP_HOME=<base> <base>/bin/tick.sh`）。

```bash
gnpc install-scheduler            # 幂等装
gnpc install-scheduler --uninstall  # 只拆调度, 不停常驻 sing-box
```

**同时清旧**（§7.1 验收 2/4 要求"无残留"）：`com.gnp.*` unload + 移走、
旧 cron 行剔除（保留新 tick 行）、`gnp-proxy`/`gnp-hy2` disable、
aipro 的 `--user sing-box` disable。

**约定**：组件**缺席 = 真静默**（设计行为，服务端没有 gnpc）；组件**失败 = 落
`var/tick.log` WARN**（不允许无声）。告警只有 guard 降级/恢复事件。

### rules-update — 规则集日更

重新下载规则集并重启 sing-box 加载。正常无需手动跑：`tick.sh` 在 04 点窗口调它，
成功才写当日标记（`var/.rules-updated-<date>`），失败落 WARN 明日再试。

```bash
gnpc rules-update          # 旧名 update-rules 仍可用 (alias)
```

下载通道：直连优先，不通自动借本地代理 `127.0.0.1:1080`
（GitHub raw 在国内直连不通，而这些机器唯一稳定的出站就是自己的代理）。

**行为说明**：

1. sing-box 未安装则下载 **v1.13.16**
2. 下载规则集（geosite-cn、geoip-cn、google、github、openai 等）
3. 从 `config.toml` 渲染 `config.json`（双通道：selector → urltest → hy2 + ssh 兜底）
4. `sing-box check` 校验生成物
5. 写 `secrets/`（0600 带换行）
6. Linux 写 systemd `gnpc.service`（开机自启）

**示例**：

```bash
gnpc install \
  --server 8.209.203.17 \
  --password <你的密码> \
  --server-port 443
```

> ⚠️ 密码需要安全传输，不要泄露。

---

### 双通道架构与自动降级（2026-10-05 新增）

`install` 生成的配置不再是单 hy2 通道，而是**双通道 + 自动选路**（2026-10-05 跨境
hy2 断网两天事件的修复，见 [finished-plans/2026-10-04-hy2-outage-repair.md](finished-plans/2026-10-04-hy2-outage-repair.md)）：

```
route.final → proxy-out (selector, 手动 override 入口)
    └→ auto-out (urltest: 每 3m 探测, tolerance 300ms)
        ├→ hy2-out  QUIC 主通道（快时胜出）
        └→ ssh-out  TCP 兜底（复用服务端 sshd, 免疫 UDP 黑洞）
```

- hy2 探测失败 → urltest 一个周期内自动落 ssh；恢复后自动回切，无需人工干预
- DNS（dns-remote）detour 跟随 selector，通道降级时 DNS 同步降级
- 配置同时开启 clash_api（127.0.0.1:9090），供 switch/status/guard 读取热状态
- ssh 兜底默认开启：密钥 `~/.ssh/id_ed25519`、user=lw、端口 22（`--no-ssh-fallback` 可关）
- 服务端 inbound 若开 salamander obfs，生成时必须 `--obfs-password <密码>`

### probe — 主动诊断（hy2 握手 + MTU 扫描）

把"代理时通时不通/无声超时"的标准排查一条命令化（排障范式见 plan 附录）：

```bash
gnpc probe                # 全套: 握手测试 + 载荷尺寸扫描
gnpc probe --skip-mtu     # 只测 hy2 握手
gnpc probe --sizes 1200,1240,1280 --count 20
```

1. **hy2 握手测试**：起临时 sing-box 实例（独立端口、无 cache_file），经真实路径
   curl generate_204，输出握手是否可用与出口 IP
2. **UDP 载荷尺寸扫描**：向服务端发 1200/1240/1280/1332/1400B 各 N 包（×3 轮取中位），
   ssh 读服务端 `/proc/net/snmp` Udp InDatagrams 差分 → MTU 截止点（需本机 ssh 免密登录服务端）
3. **结论**：明确输出"路径可容 QUIC / MTU 截止 ~xB, QUIC 初始包被丢 → 建议 ssh"

### switch — 手动通道切换

urltest 按"最快"自动选路；switch 提供 manual override：

```bash
gnpc switch        # 查看当前通道（selector 默认 + 热状态）
gnpc switch ssh    # 强制 TCP 兜底（如怀疑 QoS / hy2 半死）
gnpc switch hy2    # 强制 QUIC
gnpc switch auto   # 交还 urltest 自动选路（默认）
```

实现 = clash_api 热切换（立即生效）+ config selector default 写回（重启仍生效）。
手动切换后 guard 不会覆盖你的选择（guard 只在"自己冻结的 ssh"恢复时解冻）。

### guard — 通道看门狗（退避探测 + 故障冻结 + 告警）

设计为每分钟一次的单 tick（无常驻进程），macOS 用 launchd（com.gnp.guard）或
cron 驱动，Linux 用 cron：

```bash
gnpc guard                 # 手动跑一次 tick（调试）
gnpc guard --install-cron  # Linux 装 cron（每分钟）; macOS 建议用 launchd agent
```

行为：

1. 每 tick 经 clash_api 探测 hy2 延迟（5s 超时）
2. 失败 → 指数退避再探（60s→120s→cap 180s, ±20% 抖动），全程落 `guard.log`——
   掐掉"服务端重启 → 全员重连风暴"的二次伤害
3. 连续 2 次失败 → 冻结到 ssh-out（selector 强制）+ 重启 sing-box（确保兜底通道干净）+ 告警
4. 恢复探测通过 → 自动解冻交还 urltest + 告警
5. 告警渠道：`GNP_ALERT_CMD` 环境变量钩子 > macOS 系统通知 > Linux notify-send，
   始终落 `~/.local/gnp/var/guard.log`；持续故障 30min 限频

---

### register — 自动注册新机器

从 gitee 私有仓库的用户密码池自动取配置，一键完成安装。

```bash
# 设置 token
export GITEE_TOKEN=xxxx

# 自动注册（client_id 默认用 hostname）
gnpc register

# 指定 client_id
gnpc register --client-id macbook

# 只查看用户密码池状态，不修改
gnpc register --list

# 试运行（看会选中哪个用户，不实际修改）
gnpc register --dry-run
```

**参数说明**：

| 参数 | 说明 |
|------|------|
| `--client-id <ID>` | 客户端标识（可选，默认用 hostname） |
| `--list` | 列出用户池状态（available / used / activated） |
| `--dry-run` | 只看会选中哪个用户，不实际修改 |

**工作流程**：

1. 克隆 gitee 私有仓库（需要 `GITEE_TOKEN`）
2. 读取 `peers/` 目录下的用户 JSON
3. 选择一个 `status=available` 的用户（优先匹配 client_id）
4. 标记为 `used` 并 push 回 gitee
5. 校验 server 密码（`peers/HY2_PASSWORD`，防篡改）
6. 生成 sing-box config.json（hysteria2 outbound）
7. 下载安装 sing-box + 规则集
8. 安装 systemd 服务（Linux）
9. 验证配置

> ⚠️ **重要**：register 完成后，需要在 **server** 上执行 `gnps activate <client_id>`，将密码加入 gnp-hy2 运行时。

---

### update-rules — 规则集更新 + 守护

更新 sing-box 规则集（geosite/geoip），或检查守护进程。

```bash
# 检查 sing-box 是否运行，挂了就重启（默认行为）
gnpc update-rules
# 等价于
gnpc update-rules --check

# 强制更新规则集（重启 sing-box 加载最新 remote rule-set）
gnpc update-rules --update

# 安装 cron 任务（每天 04:00 自动检查）
gnpc update-rules --install-cron
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
* * * * * GNP_HOME=/home/<user>/.local/gnp /home/<user>/.local/gnp/bin/tick.sh
```

---

### cleanup — 应急清理

彻底清理 sing-box 所有残留。参考 [aipro 断网事故](incident-2026-08-10.md)。

```bash
gnpc cleanup
```

**清理步骤（6 步）**：

1. **停止服务**：systemctl stop / launchctl unload / pkill -9 sing-box
2. **禁用开机自启**：systemctl disable / mask
3. **清理 tun 接口**：删除 gnp0、tun0
4. **清理策略路由**：删除 priority 9000-9010 的 ip rule，flush table 2022
5. **恢复默认路由**：探测网关并恢复
6. **备份数据目录**：`~/.local/gnp/` → `gnp.disabled-<timestamp>`

> ⚠️ 这条命令会**彻底清除** sing-box，之后需要重新 `gnpc install` 或 `register`。

---

### recover — 断网恢复

sing-box tun 模式破坏路由表后的网络恢复工具。

```bash
gnpc recover
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
gnpc proxy --status

# 开启系统代理
gnpc proxy --on

# 关闭系统代理
gnpc proxy --off

# 无参数：显示状态和用法
gnpc proxy
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

## 一键接入：gen-user → peer（2026-08-15 新增）

```bash
# server 端（lwtop）：
sudo gnps gen-user --name macbook          # 生成密码+写入配置+重启 hy2+落盘 gnp.cfg
sudo gnps gen-user --name vmwin --out /tmp/vmwin.cfg

# 客户端（任意平台）：
gnpc peer gnp.cfg                          # 解析 cfg → 装 sing-box/规则/生成配置/装服务
gnpc start && gnpc test              # 上线验证
```

gnp.cfg 字段：`user-name / server-ip / server-port / peer-key`
⚠️ peer 会覆盖 config.json——手工精调过的节点（如 deploy/config-mac）先备份。

## gnps 命令详解

gnps 管理 Hysteria2 (QUIC) server（sing-box hysteria2 inbound，systemd 服务 `gnp-hy2`），共 **7 个子命令**。

> **所有命令需要 root 权限**（写 `/opt/gnp`、控制 systemd），请用 `sudo` 运行。
>
> **唯一事实源**：`/opt/gnp/config.toml`（`config.json` 由它渲染，勿手改）
> **端口**：443（UDP/QUIC）
> **服务**：`gnp-hy2`
> **证书**：`/opt/gnp/certs/`（自签）

---

### install — 安装 Hysteria2 server

安装 Hysteria2 (QUIC) server，包括 sing-box 二进制、自签证书、配置、systemd 服务、防火墙放行。

```bash
sudo gnps install
```

**安装步骤**：

1. **下载 sing-box**（with_quic 构建）到 `/opt/gnp/bin/sing-box`
2. **生成自签证书**：`openssl req -x509` 生成 `/opt/gnp/certs/server.crt` / `server.key`（10 年有效期）
3. **渲染 config.json**：从 `config.toml` 渲染（hysteria2 inbound，listen 5766，users 来自 `[[users]]`），并跑 `sing-box check`
4. **写 systemd 单元**：`/etc/systemd/system/gnp-hy2.service`（`sing-box run -c <config>`）
5. **启动服务**：`systemctl enable --now gnp-hy2`
6. **放行 UDP 443**：`iptables -I INPUT -p udp --dport 443 -j ACCEPT`

**安装完成后**：

```
✅ Server 部署完成!
  server: 8.209.203.17:443 (UDP/QUIC)
  证书: /opt/gnp/certs/server.crt / server.key
  配置: /opt/gnp/config.json
  服务: gnp-hy2 (systemd)

下一步: gnps add-user <名称> 添加用户
```

> ⚠️ 阿里云安全组需要放行 **UDP 443** 端口。

---

### uninstall — 卸载 server

完全卸载 Hysteria2 server。

```bash
sudo gnps uninstall
```

**卸载内容**：

1. 停止并禁用 `gnp-hy2` 服务
2. **保留** `/opt/gnp/` 数据（config.toml / 证书 / users / pending-users）：卸载只停服务删单元，要彻底清再手动删该目录
3. 删除 systemd 单元 `/etc/systemd/system/gnp-hy2.service`
4. `systemctl daemon-reload`

---

### status — 查看状态

查看 gnp-hy2 服务状态和用户信息。

```bash
sudo gnps status
```

**输出内容**：

- gnp-hy2 是否激活
- UDP 443 是否监听
- 服务详情（systemd 状态）

---

### users — 列出用户

列出所有已注册的用户密码。

```bash
sudo gnps users
```

**输出**：config.json 中 users[] 的所有密码列表。

---

### add-user — 添加用户

为新客户端生成密码并加入 gnp-hy2。

```bash
sudo gnps add-user <名称>
```

**参数**：

| 参数 | 说明 |
|------|------|
| `name` | 客户端名称（如 macbook、aipro、win-01） |

**行为说明**：

1. 生成随机密码（`gnp-<hex>`）
2. 将密码追加到 config.json 的 users[]
3. 重启 gnp-hy2 服务使其生效
4. 输出客户端连接信息

**输出示例**：

```
== 添加用户: macbook ==
  password: gnp-xxxxxxxx

✅ 用户已添加!
==================
server: 8.209.203.17:443
password: gnp-xxxxxxxx
==================
```

> ⚠️ 输出的密码需要安全传输到客户端机器，不要泄露。

---

### pregen — 预生成用户密码池

批量生成待用用户密码包，不占运行时资源。

```bash
sudo gnps pregen <数量>
```

**参数**：

| 参数 | 说明 |
|------|------|
| `count` | 要预生成的用户数量 |

**行为说明**：

- 为每个用户生成唯一密码
- 存为 JSON 到 `/opt/gnp/pending-users/<id>.json`
- JSON 包含：id、status=available、password、server_endpoint `8.209.203.17:443`
- 文件权限 600

**用途**：配合 `gnpc register` 实现新机器自动注册。用户密码池可推送到 gitee 私有仓库。

---

### activate — 激活预生成的用户

将 pending-users 中的用户密码加入 gnp-hy2 运行时。

```bash
sudo gnps activate <client_id>
```

**参数**：

| 参数 | 说明 |
|------|------|
| `id` | 用户的 client_id（pregen 时生成的 ID） |

**行为说明**：

1. 从 `/opt/gnp/pending-users/<id>.json` 读取配置
2. 将密码追加到 config.json 的 users[]（若已存在则跳过）
3. 重启 gnp-hy2 服务
4. 更新 JSON 状态为 `activated`

> ⚠️ `gnpc register` 完成后，**必须**在 server 上执行此命令，否则客户端无法连通。

---

## 典型场景

### 场景一：首次部署（从零开始）

**在 Server 上（海外节点）**：

```bash
# 1. 安装 gnp CLI
bash bash/install.sh

# 2. 安装 Hysteria2 server
sudo gnps install

# 3. 预生成用户密码池（推荐）
sudo gnps pregen 20

# 4. 将 pending-users/ 目录推送到 gitee 私有仓库
cd /opt/gnp/pending-users/
# 复制到项目仓库的 peers/ 目录，push 到 gitee
```

**在 Client 上（每台机器）**：

```bash
# 1. 安装 gnp CLI
bash bash/install.sh

# 2. 自动注册
export GITEE_TOKEN=xxxx
gnpc register my-machine

# 3. 回到 Server 上激活
sudo gnps activate my-machine

# 4. 启动代理
gnpc start

# 5. 设置系统代理
# macOS:
gnpc proxy --on
# Linux:
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080

# 6. 验证
gnpc test
```

### 场景二：新机器加入

```bash
# 在新机器上
export GITEE_TOKEN=xxxx
gnpc register new-machine-id
# → 自动取用户密码、生成配置、安装 sing-box

# 在 Server 上激活
sudo gnps activate new-machine-id

# 启动
gnpc start
```

### 场景三：日常使用

```bash
# 每天开机后代理已自动启动（systemd/launchd 开机自启）
# 只需设置代理：

# macOS（浏览器）
gnpc proxy --on

# Linux（终端）
export http_proxy=http://127.0.0.1:1080 https_proxy=http://127.0.0.1:1080

# 查看状态
gnpc status

# 测试连通性
gnpc test
```

### 场景四：故障排查

```bash
# 1. 查看完整状态
gnpc status

# 2. 检查配置安全
gnpc config --check

# 3. 隧道诊断
gnpc tunnel

# 4. 如果代理不工作，尝试重启
gnpc stop
gnpc start

# 5. 如果断网了（tun 模式残留）
gnpc recover    # 恢复网络
gnpc cleanup    # 彻底清理 sing-box
# 然后重新安装
gnpc install ...
gnpc start
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
| 系统代理 | `gnpc proxy --on`（networksetup + osascript） | `gnpc proxy --on`（gsettings）或 export |
| 授权方式 | osascript 弹窗（不存密码） | 无需授权（gsettings 或环境变量） |
| 浏览器生效 | Safari/Chrome 自动走系统代理 | GNOME 应用走 gsettings；终端需 export |
| 终端代理 | 需手动 export | `export http_proxy=http://127.0.0.1:1080` |

### 部署目录（跨平台一致，2026-10-05 起）

客户端 `~/.local/gnp/`（`$GNP_HOME` 可覆盖；服务端 `/opt/gnp/`）：

```
~/.local/gnp/
├── bin/
│   ├── gnpc              # 客户端 CLI (tick.sh 的内核组件)
│   ├── sing-box          # 二进制
│   └── tick.sh           # 唯一调度入口 (全部机器同一份)
├── config.toml           # ★ 唯一事实源 (0600, 含内联密码)
├── config.json           # 生成物 (勿手改)
├── etc/tick.d/*.sh       # 可选组件: 存在即跑, 缺席静默
├── rules/*.srs           # 规则集 (geosite-cn / geoip-cn / google / github / ...)
├── secrets/              # hy2-password, hy2-obfs (0600 带换行; 容器挂载源)
├── var/                  # cache.db, guard-state.json, guard.log, tick.log,
│                         # sing-box.log/.err (状态与日志, 勿放配置)
└── backups/              # 历次变更快照 + migrate 后的 legacy 整目录 (回滚用)
```

---

## 技术细节

### sing-box 版本

使用 **sing-box v1.13.16**（with_quic 构建，原生支持 hysteria2 outbound）。

> 下载时需带 `with_quic` 特性构建/下载，否则 hysteria2 outbound 不可用。

### Hysteria2 outbound 格式

使用 **outbound hysteria2**（1.13 格式），通过密码认证。

配置片段：

```json
{
  "outbounds": [{
    "type": "hysteria2",
    "tag": "hy2-out",
    "server": "8.209.203.17",
    "server_port": 443,
    "password": "<HY2_PASSWORD>",
    "tls": { "enabled": true, "insecure": true }
  }]
}
```

- `password`：Hysteria2 认证密码（由 `gnps add-user` 生成）
- `tls.insecure: true`：信任 server 自签证书
- 基于 QUIC/TLS 1.3，无需像 WireGuard 那样维护公钥对和虚拟 IP

### 端口 443

使用 **UDP 443**（QUIC），替代旧方案的 1194。

> 443 端口通常是放行的（HTTPS 同端口），且 QUIC 流量难以被深度检测，抗封锁能力更强。

需要在云服务商安全组放行 **UDP 443**。

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
| 国外域名 | `1.1.1.1` | 经 hy2 隧道（detour=hy2-out，TCP） |

- 国外域名 DNS 经 hysteria2 隧道走 1.1.1.1 **TCP**，避免 DNS 污染
- 使用 `socks5h`（带 h）进行 HTTP 代理时，DNS 在代理端远程解析

### 路由规则

```json
{
  "route": {
    "rules": [
      { "ip_is_private": true, "outbound": "direct" },
      { "rule_set": "geosite-google", "outbound": "hy2-out" },
      { "rule_set": "geosite-github", "outbound": "hy2-out" },
      { "rule_set": "geosite-openai", "outbound": "hy2-out" },
      { "rule_set": "geosite-anthropic", "outbound": "hy2-out" },
      { "rule_set": "geosite-docker", "outbound": "hy2-out" },
      { "rule_set": ["geoip-cn", "geosite-cn"], "outbound": "direct" }
    ],
    "final": "hy2-out"
  }
}
```

| 流量类型 | 判定 | 出口 |
|---------|------|------|
| 国内域名/IP | geoip-cn / geosite-cn | direct（直连） |
| 私有 IP | ip_is_private | direct（直连） |
| 国外主流站点 | geosite-google/github/openai 等 | hy2-out（走隧道） |
| 其余所有 | final | hy2-out（走隧道） |

### Server 信息

| 项目 | 值 |
|------|------|
| Server 地址 | `8.209.203.17:443`（UDP/QUIC） |
| 认证 | Hysteria2 密码（`HY2_PASSWORD`） |
| 证书 | 自签 `/opt/gnp/certs/`（客户端 insecure 信任） |
| 端口 | UDP 5766（`GNP_PORT`，代码默认值；改动需 ufw + 云安全组双侧） |

> 密码需要安全传输，不影响安全性。密码池存在 gitee 私有仓库。

---

> 📖 相关文档：[架构设计](architecture.md) | [自动注册](auto-registration.md) | [断网事故记录](incident-2026-08-10.md) | [已完成计划归档](finished-plans/README.md)