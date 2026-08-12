# 预生成用户（密码池）自动注册方案

## 目标

新机器接入 global-net-proxy 时，**无需管理员手动在 lwtop 上操作**，即可一键完成用户注册和 sing-box 安装。

核心思路：**server 预生成 N 个 Hysteria2 用户密码包，存入 gitee 私有仓库；新机器从池中取一个未使用的，自动生成配置并安装。**

> 相比旧版 WireGuard 方案（公钥对 + IP 分配），Hysteria2 只需一个**密码**就能认证，注册流程大幅简化：无需生成/交换公钥，无需分配虚拟 IP。

---

## 方案总览

### 角色

| 角色 | 位置 | 职责 |
|------|------|------|
| **lwtop server** | 海外 VPS (8.209.203.17) | hysteria2 server (gnp-hy2)，用户激活 (`--activate`)，预生成 (`--pre-gen`) |
| **gitee 私有仓库** | lw_boy/global-net-proxy | 存储预生成的用户密码池（含密码），server 密码校验值 |
| **新机器 client** | aipro/mac/ningsure/lwpc 等 | 运行 `gnp-client register`，从池中取密码，安装 sing-box |

### 数据流

```
 ┌─────────────────────────────────────────────────────────────────┐
 │  lwtop server                                                   │
 │                                                                 │
 │  gnp-server pregen 20                                         │
 │    ├─ 生成 20 个用户密码包（id, password, status）              │
 │    ├─ 存到 /opt/gnp-quic/pending-users/<id>.json               │
 │    └─ git push → gitee 私有仓库 peers/ 目录                     │
 │                                                                 │
 │  gnp-server activate aipro                                     │
 │    ├─ 读 pending-users/aipro.json                              │
 │    ├─ 将 password 加入 config.json 的 users[]                  │
 │    ├─ 重启 gnp-hy2 服务                                        │
 │    └─ 标记该用户为 activated（status: "activated"）             │
 └────────────────────┬────────────────────────────────────────────┘
                      │ git push (私有仓库)
                      ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │  gitee 私有仓库 (lw_boy/global-net-proxy)                       │
 │                                                                 │
 │  peers/                                                         │
 │    ├─ aipro.json          ← status: "used", activated: true     │
 │    ├─ mac.json            ← status: "used", activated: true     │
 │    ├─ slot-03.json        ← status: "available"                 │
 │    ├─ slot-04.json        ← status: "available"                 │
 │    └─ ...                                                       │
 │                                                                 │
 │  HY2_PASSWORD             ← 文本文件，密码校验值可公开           │
 └────────────────────┬────────────────────────────────────────────┘
                      │ git clone (需 GITEE_TOKEN)
                      ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │  新机器 (e.g. ningsure)                                         │
 │                                                                 │
 │  gnp-client register ningsure                                           │
 │    1. git clone https://<token>@gitee.com/lw_boy/global-net-proxy│
 │    2. 找到 status="available" 的 peer → 改为 "used"              │
 │    3. git push 回 gitee（标记占用）                              │
 │    4. 校验 peers/HY2_PASSWORD                                    │
 │    5. 生成 ~/.local/share/sing-box/config.json (mixed 模式,     │
 │       hysteria2 outbound, 填入 password)                         │
 │    6. 下载 sing-box + 安装 systemd service                      │
 │    7. 提示用户: 去 lwtop 跑 gnp-server activate ningsure       │
 └─────────────────────────────────────────────────────────────────┘
```

---

## 详细流程

### Phase 1: Server 预生成 (`gnp-server pregen <N>`)

```
[管理员在 lwtop 上运行]
sudo gnp-server pregen 20

  1. 检查 root + gnp-hy2 已安装
  2. 扫描 /opt/gnp-quic/pending-users/ 和 config.json 现有 users
  3. for i in 1..N:
       a. 生成 id: 自动分配 slot-NN
       b. 生成唯一密码 password (gnp-<hex>，随机)
       c. 写 JSON:
          {
            "id": "slot-01",
            "status": "available",
            "client_id": "",
            "password": "gnp-xxxxxxxx",
            "server_endpoint": "8.209.203.17:443",
            "created": "2026-08-10T12:00:00Z"
          }
  4. 汇总输出
  5. 可选: --push 参数 → git commit + push 到 gitee
```

### Phase 2: Client 注册 (`gnp-client register <client_id>`)

```
[新机器上运行]
gnp-client register ningsure

  1. 检测平台 + 架构
  2. 克隆 gitee 私有仓库到临时目录
     git clone --depth 1 https://oauth2:<token>@gitee.com/lw_boy/global-net-proxy
  3. 在 peers/ 目录中:
     a. 优先找 client_id 匹配的文件 (ningsure.json)
     b. 若不存在, 找任意 status="available" 的 peer
     c. 若都不可用, 报错退出
  4. 将选中 peer 的 status 改为 "used", 写入 client_id (若原为 slot)
     → git commit + push 回 gitee (标记占用)
  5. 读取 peers/HY2_PASSWORD 校验 server 密码
  6. 生成 sing-box config.json:
     - 用 safe-template.json 结构
     - 填入: server (8.209.203.17), server_port (443), password
     - hysteria2 outbound + mixed 模式 (socks5+http 0.0.0.0:1080)
  7. 下载 sing-box (with_quic)
  8. 安装 systemd 系统服务 (/etc/systemd/system/gnp-proxy.service)
  9. 提示:
     ═══════════════════════════════════════════
     ⚠️  最后一步: 在 lwtop 上执行激活!
     
     ssh lwtop
     sudo gnp-server activate ningsure
     ═══════════════════════════════════════════
```

### Phase 3: Server 激活 (`gnp-server activate <client_id>`)

```
[管理员在 lwtop 上运行]
sudo gnp-server activate ningsure

  1. 检查 root + gnp-hy2 运行中
  2. 读 /opt/gnp-quic/pending-users/ningsure.json
     → 提取 password
  3. 检查该密码是否已在 config.json 的 users[] (防重复激活)
  4. 将 password 追加到 config.json 的 users[]
     (写入后重启 gnp-hy2 服务生效)
  5. 标记 JSON: status="activated"
  6. 可选: --push → git push 更新状态
  7. 输出确认信息
```

---

## 用户 JSON 格式

```json
{
  "id": "slot-01",
  "status": "available | used | activated",
  "client_id": "ningsure",
  "password": "gnp-xxxxxxxx",
  "server_endpoint": "8.209.203.17:443",
  "created": "2026-08-10T12:00:00Z"
}
```

状态流转:
```
available  ──gnp-client register 取用──▶  used  ──gnp-server activate──▶  activated
```

---

## 安全分析

### 威胁模型

| 威胁 | 风险 | 缓解措施 |
|------|------|---------|
| 用户密码泄露 | 攻击者可冒充 client 连接 server | gitee **私有**仓库 + token 认证；密码不出 gitee |
| 未经授权的机器注册 | 任意机器获取用户密码 | GITEE_TOKEN 是认证边界；token 只给可信机器 |
| 密码重用（同一密码多机使用） | 连接冲突 + 密码泄露 | gnp-client register 原子标记 `status: used` 并 push 回 gitee |
| server 密码校验值泄露 | 无风险（仅用于校验） | 校验值写在 readme.md，任何人可见 |
| gitee token 泄露 | 所有用户密码泄露 | token 权限最小化（只读该仓库）；定期轮换 |
| man-in-the-middle 注册过程 | client 拿到错误的 server 密码 | peers/HY2_PASSWORD 硬编码在 gnp-client register 中作为校验值 |

### 安全原则

1. **密码始终在 gitee 私有仓库**：gnp-client register 拉取后写入本地，不从公开渠道传输
2. **server 密码校验值可公开**：校验值不影响安全性，写在 readme.md 方便校验
3. **用户配置一次性**：`status` 字段保证每个密码只被一台机器使用
4. **token 是唯一认证**：GITEE_TOKEN 控制谁能拉取密码池，等于"谁能加入网络"
5. **激活分离**：gnp-client register 只完成 client 侧配置；server 侧激活需要管理员手动操作（防止恶意注册直接上线）

### 信任边界

```
[可信区]                          [半可信区]                    [不可信区]
 lwtop server                     gitee 私有仓库                 公网
 (持有 server 配置 + 证书)        (持有用户密码池)             
  │                                │
  │  GITEE_TOKEN 是信任传递的桥梁    │
  └────────────────────────────────┘
                │
                ▼
         可信 client 机器
         (持有自己的用户密码)
```

---

## 认证与连接说明

- **协议**：Hysteria2 (QUIC over UDP 443，TLS 1.3 加密)
- **认证**：仅需密码（`HY2_PASSWORD`），无需公钥对、无需分配虚拟 IP
- **证书**：server 自签证书 `/opt/gnp-quic/certs/`，客户端 `tls.insecure: true` 信任
- **端口**：UDP 443

相比旧版 WireGuard：**每个用户只需一个密码**，无需生成/交换公钥、无需管理虚拟 IP 分配，注册和激活流程更简单。

---

## 文件结构

```
global-net-proxy/
├── crates/
│   ├── gnp-core/              # 共享库
│   ├── gnp-client/            # client CLI (register 等)
│   └── gnp-server/            # server CLI (pregen/activate)
├── config/
│   └── safe-template.json
├── docs/
│   └── auto-registration.md   # 本文档
├── bash/
│   ├── install.sh             # 构建 + 安装所有依赖
│   ├── uninstall.sh           # 卸载
│   └── client/                # (应急) 断网兜底脚本
└── peers/                     # server 密码校验值 + 用户池（gitee 私有）
    └── HY2_PASSWORD           # server 密码校验值文本 (可公开)
    # slot-*.json              # 用户池 (含密码), 只存 gitee 私有仓库
```

---

## 使用示例

### 管理员：预生成 20 个用户

```bash
# 在 lwtop 上
sudo gnp-server pregen 20
# 输出: 生成了 slot-01..slot-20, 存在 /opt/gnp-quic/pending-users/

sudo bash gnp-server pregen 5 --push
# 生成并推送到 gitee
```

### 新机器：一键注册

```bash
# 在 ningsure 上 (有 GITEE_TOKEN)
export GITEE_TOKEN=xxxx
gnp-client register ningsure

# 输出:
# [INFO] 从 gitee 拉取用户池...
# [INFO] 选中用户: ningsure (password: gnp-xxxx)
# [INFO] 标记已使用...
# [INFO] 生成配置: ~/.local/share/sing-box/config.json
# [INFO] 安装 sing-box (with_quic)...
# [INFO] 安装 systemd service...
# ═══════════════════════════════════════════
# ⚠️  最后一步: 在 lwtop 上执行激活!
#   ssh lwtop
#   sudo gnp-server activate ningsure
# ═══════════════════════════════════════════
```

### 管理员：激活

```bash
# 在 lwtop 上
sudo gnp-server activate ningsure
# 输出:
# [INFO] 激活用户: ningsure
# [INFO] 将密码加入 config.json users[] 并重启 gnp-hy2
# [INFO] ✓ 用户 ningsure 已激活
```

### 验证

```bash
# 在 client 上
gnp-client test       # 测试连通性
gnp-client start      # 启动
curl -x socks5h://127.0.0.1:1080 https://ifconfig.me
# 应返回 lwtop 的公网 IP
```