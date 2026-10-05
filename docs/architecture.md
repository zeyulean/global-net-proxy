# global-net-proxy 架构

> 2026-10-05 v2 重构后形态（gnpc/gnps · config.toml 唯一事实源 · tick.sh 调度 · 路径收敛）。
> 实施过程与坑清单见 [finished-plans/2026-10-05-v2-refactor.md](finished-plans/2026-10-05-v2-refactor.md)。

## 一句话

国内外网络分流：**国外流量（github/google/代码源/AI）通过 mixed 代理端口走 Hysteria2 (QUIC) 隧道经 lwtop 海外出口，国内流量直连**；hy2 故障时自动降级 ssh (TCP) 兜底，恢复后自动回切。

## ⚠️ 安全原则

### 为什么不用 tun 模式

tun 模式（`strict_route: true` + `auto_route: true`）会让 sing-box **接管系统路由表**。这在以下场景是灾难性的：

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
| 透明代理 | 是（系统级） | 否（需设置 http_proxy；aipro AP 路由容器除外） |
| 使用方式 | 无感 | `export http_proxy=http://127.0.0.1:1080` |

mixed 模式只监听 `1080`（同时支持 socks5 和 http 代理），不创建 tun 设备，不修改路由表。即使配置错误，也不会影响系统网络。

## 拓扑（双通道）

```
  客户端 (Mac / Linux / Windows)
  ┌──────────────────────────────────┐                          ┌────────────────────────────┐
  │  sing-box (gnpc.service 常驻)     │  hy2: QUIC/UDP 5766+salamander │  lwtop (8.209.203.17)   │
  │  ├─ mixed-in (:1080)             │◀────────────────────────▶│  hy2-in (gnps.service)     │
  │  ├─ route.final → proxy-out      │  ssh: TCP 22 (密钥) 兜底   │  ├─ 8 用户密码认证          │
  │  │    selector{auto,hy2,ssh}     │◀─────────────────────────▶│  └─ 出口: 海外公网          │
  │  ├─ auto-out = urltest(hy2,ssh)  │                          └────────────────────────────┘
  │  └─ DNS 分流 (国内直连/国外走隧道) │
  │  gnpc guard (tick.sh 每分钟)      │  探测失败×2 → 冻结 ssh-out + 告警; 恢复 → 自动回切 hy2
  └──────────────────────────────────┘
```

两条通道：

| 通道 | 协议 | 用途 |
|------|------|------|
| `hy2-out` | hysteria2 (QUIC/UDP **5766**, salamander obfs) | 主通道，低延迟 |
| `ssh-out` | ssh (TCP 22, lwtop 密钥认证) | 兜底通道，免疫 UDP 封锁/QoS |

selector `proxy-out` 默认指 `auto-out`（urltest 自动选路）；guard 在 hy2 连续失败时强制 `ssh-out`，恢复后交还 `auto-out`。

## 组件分工

| 组件 | 端 | 作用 |
|------|----|----|
| `gnps.service` (sing-box, lwtop root) | **server** | hysteria2 入站 + 密码认证 + NAT 转发，UDP 5766 |
| `gnpc.service` / launchd `com.gnpc.singbox` (sing-box 常驻) | **client** | mixed 代理端口 + DNS 分流 + 双通道 outbound + route 规则 |
| `gnpc` (Rust CLI, bin 名; crate 仍叫 gnp-client) | **client** | install/migrate/status/probe/switch/guard/register/rules-update/install-scheduler |
| `gnps` (Rust CLI, bin 名; crate 仍叫 gnp-server) | **server** | install/users/gen-user/pregen/activate |
| `tick.sh` (bash, 全机同一份) | 两者 | 调度唯一入口（cron/launchd 每分钟调）：内核组件 `gnpc guard` + `rules-update`(04 点) + `etc/tick.d/*.sh` 插件 |
| geosite/geoip 规则集 (.srs) | client | 国内外分流依据，`gnpc rules-update` 日更 |

## 部署布局（v2 路径收敛）

```
客户端 ~/.local/gnp/                 服务端 /opt/gnp/ (lwtop)
  bin/{gnpc, sing-box, tick.sh}       bin/{gnps, sing-box, tick.sh}
  config.toml        ← 唯一事实源     config.toml        ← 唯一事实源 (8 用户密码内联)
  config.json        (生成物)         config.json        (生成物)
  etc/tick.d/*.sh    (可选插件)       etc/tick.d/gnps-health.sh
  rules/*.srs                         certs/server.crt|key
  var/               (状态/日志)      var/, backups/
  secrets/           (hy2-password, hy2-obfs)
  backups/           (含 legacy-singbox/)
```

每台机器的 config.toml 在 repo `deploy/hosts/<host>.toml` 有一份 canonical 版本（**md5 必须一致**，验收项）；所有密码内联进 git（私有 repo，D2 决策）。

## 服务端要点 (lwtop)

- **配置**：`/opt/gnp/config.toml`（事实源）→ 渲染 `config.json`（生成物，勿手改）
- **服务**：systemd `gnps.service`（root；旧 `gnp-hy2` 已退役进 backups）
- **证书**：`/opt/gnp/certs/server.crt` / `server.key`（自签，10 年）
- **端口**：`5766`（UDP/QUIC + salamander obfs）；改动必须 **ufw + 云安全组双侧**（禁裸 iptables）
- **认证**：`[[users]]` 数组（`gnps gen-user` / `pregen` / `activate` 管理）
- **调度**：root crontab 一行 `GNP_HOME=/opt/gnp /opt/gnp/bin/tick.sh`；tick.d 的 gnps-health.sh 自动拉起挂掉的服务

## 客户端要点

sing-box **1.13.16**。`gnpc install` / `gnpc migrate` 从 config.toml 生成 config.json（`sing-box check` 不过不许上线）：

1. **双通道 outbound**：`hy2-out`（QUIC）+ `ssh-out`（TCP 兜底）→ `auto-out`（urltest, tolerance 300ms）→ `proxy-out`（selector）
2. **mixed inbound**：socks5 + http，`listen` 由 toml 决定（Mac=127.0.0.1 单机自用，服务机=0.0.0.0 局域网）
3. **clash_api**：`127.0.0.1:9090`，热切换/延迟探测/probe 的接口
4. **DNS**：国内 223.5.5.5 直连，国外走隧道（detour 跟随 route.final）
5. **route.final = proxy-out**；cache_file 必须绝对路径（systemd CWD=/ 不可写，aipro crash-loop 111 次教训）
6. **看门狗**：`tick.sh` → `gnpc guard`，指数退避探测（60s→cap 180s ±20% 抖动），连续 2 败冻结 ssh + 重启 sing-box + 告警，恢复自动回切 + 纠正 urltest tolerance 粘滞

## 分流原则

| 流量 | 判定规则 | 出口 |
|------|---------|------|
| github/google/openai/pypi/npm/crates/go/maven/docker 等 | geosite 命中 | 走隧道 → lwtop 海外 |
| 国内域名/国内 IP | geosite-cn / geoip-cn 命中 | direct 直连 |
| 私有 IP (10.x/172.16.x/192.168.x) | ip_is_private | direct 直连 |
| `[client.hosts]` 预定义 | 域名规则 | direct（Mac 专用） |
| 其余未知 | final=proxy-out | 走隧道（安全默认） |

## 目录结构

```
global-net-proxy/
├── Cargo.toml              # Cargo workspace 根
├── crates/
│   ├── gnp-core/           # 共享库: 平台/路径/settings(toml)/config/scheduler(资产嵌入)/服务管理
│   ├── gnp-client/         # gnpc: install/migrate/probe/switch/guard/register/rules-update/install-scheduler...
│   └── gnp-server/         # gnps: install/users/gen-user/pregen/activate
├── deploy/                 # 个人部署资产 (私人 repo 部分)
│   ├── hosts/              # ★ 每台机器的 config.toml canonical 版本 (md5 验收基准)
│   ├── scheduler/          # tick.sh + plist/unit/cron 资产 (include_str! 编进二进制, 改后需重编)
│   ├── aipro-wifi/         # AP 路由容器 (tproxy 透明代理, 挂载宿主 gnp secrets)
│   ├── ns-hub/             # wg mesh 配置
│   ├── quic-test/          # 服务端模板与排障手册
│   └── peers/              # HY2_PASSWORD 校验值等
├── vendor/sing-box/        # submodule: sing-box 源码
├── bash/                   # 应急兜底脚本 (断网时 Rust 二进制无法运行)
├── config/safe-template.json
└── docs/
    ├── usage.md            # ★ 使用手册 (日常操作看这个)
    ├── architecture.md     # 本文档
    ├── auto-registration.md
    ├── incident-*.md       # 历史事故记录 (坑的出处)
    ├── finished-plans/     # 已完成的实施计划归档
    └── archive/            # 被取代的旧文档归档
```

> bash/ 仅保留应急兜底脚本。正式管理使用 Rust CLI (`gnpc` / `gnps`)；日常操作详见 [usage.md](usage.md)。
