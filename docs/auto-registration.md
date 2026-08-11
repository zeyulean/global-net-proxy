# 预生成 Peer 自动注册方案

## 目标

新机器接入 global-net-proxy 时，**无需管理员手动在 lwtop 上操作**，即可一键完成 peer 注册和 sing-box 安装。

核心思路：**server 预生成 N 个 peer 配置包，存入 gitee 私有仓库；新机器从池中取一个未使用的，自动生成配置并安装。**

---

## 方案总览

### 角色

| 角色 | 位置 | 职责 |
|------|------|------|
| **lwtop server** | 海外 VPS (8.209.203.17) | wg0 server，peer 激活 (`--activate`)，预生成 (`--pre-gen`) |
| **gitee 私有仓库** | lw_boy/global-net-proxy | 存储预生成的 peer 配置池（含私钥），server 公钥 |
| **新机器 client** | aipro/mac/ningsure/lwpc 等 | 运行 `gnp-client register`，从池中取 peer，安装 sing-box |

### 数据流

```
 ┌─────────────────────────────────────────────────────────────────┐
 │  lwtop server                                                   │
 │                                                                 │
 │  gnp-server pregen 20                                         │
 │    ├─ 生成 20 个 peer（client_id, wg_ip, privkey, pubkey）       │
 │    ├─ 存到 /etc/wireguard/pending-peers/<client_id>.json        │
 │    └─ git push → gitee 私有仓库 peers/ 目录                     │
 │                                                                 │
 │  gnp-server activate aipro                                     │
 │    ├─ 读 pending-peers/aipro.json                              │
 │    ├─ wg set wg0 peer <pubkey> allowed-ips <wg_ip>/32          │
 │    ├─ 追加到 wg0.conf                                           │
 │    └─ 标记该 peer 为 activated（改名 .activated）                │
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
 │  SERVER_PUBKEY             ← 文本文件，公钥可公开               │
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
 │    4. 读 SERVER_PUBKEY 文件                                      │
 │    5. 生成 ~/.local/share/sing-box/config.json (mixed 模式)     │
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

  1. 检查 root + wg 已安装
  2. 扫描 /etc/wireguard/pending-peers/ 和 wg0.conf
     → 确定已占用的 IP 列表
  3. for i in 1..N:
       a. 生成 client_id: 若有 --name-prefix 则用前缀编号 (slot-01..slot-N)
          否则自动分配 slot-NN
       b. 生成私钥/公钥: wg genkey | wg pubkey
       c. 分配 wg_ip: 10.0.0.<next_available> / 10.0.0.2 ~ 10.0.0.250
       d. 写 JSON:
          {
            "client_id": "slot-01",
            "wg_ip": "10.0.0.10/32",
            "private_key": "<base64>",
            "public_key": "<base64>",
            "status": "available",
            "activated": false,
            "created_at": "2026-08-10T12:00:00Z"
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
  5. 读 SERVER_PUBKEY 文件获取 server 公钥
  6. 生成 sing-box config.json:
     - 用 safe-template.json 结构
     - 填入: private_key, address (wg_ip), public_key (server), server+port
     - mixed 模式 (socks5+http 0.0.0.0:1080)
  7. 下载 sing-box v1.13.16
  8. 安装 systemd --user service
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

  1. 检查 root + wg0 运行中
  2. 读 /etc/wireguard/pending-peers/ningsure.json
     → 提取 public_key, wg_ip
  3. 检查是否已在 wg0.conf 中 (防重复激活)
  4. wg set wg0 peer <pubkey> allowed-ips <wg_ip>/32
     (runtime 热添加, 不需要重启 wg0)
  5. 追加 [Peer] 段到 wg0.conf (持久化)
  6. 标记 JSON: activated=true, activated_at=timestamp
  7. 可选: --push → git push 更新状态
  8. 输出确认信息
```

---

## Peer JSON 格式

```json
{
  "client_id": "ningsure",
  "wg_ip": "10.0.0.15/32",
  "private_key": "base64encodedPrivateKey==",
  "public_key": "base64encodedPublicKey==",
  "status": "available | used | activated",
  "activated": false,
  "created_at": "2026-08-10T12:00:00Z",
  "activated_at": null
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
| peer 私钥泄露 | 攻击者可冒充 client 连接 wg | gitee **私有**仓库 + token 认证；私钥不出 gitee |
| 未经授权的机器注册 | 任意机器获取 peer 配置 | GITEE_TOKEN 是认证边界；token 只给可信机器 |
| peer 重用（同一配置多机使用） | IP 冲突 + 密钥泄露 | gnp-client register 原子标记 `status: used` 并 push 回 gitee |
| server 公钥泄露 | 无风险（公钥本就公开） | 公钥写在 readme.md，任何人可见 |
| gitee token 泄露 | 所有 peer 私钥泄露 | token 权限最小化（只读该仓库）；定期轮换 |
| man-in-the-middle 注册过程 | client 拿到错误的 server 公钥 | SERVER_PUBKEY 硬编码在 gnp-client register 中作为校验值 |

### 安全原则

1. **私钥始终在 gitee 私有仓库**：gnp-client register 拉取后写入本地，不从公开渠道传输
2. **server 公钥可公开**：公钥不影响安全性，写在 readme.md 方便校验
3. **peer 配置一次性**：`status` 字段保证每个 peer 只被一台机器使用
4. **token 是唯一认证**：GITEE_TOKEN 控制谁能拉取 peer 池，等于"谁能加入网络"
5. **激活分离**：gnp-client register 只完成 client 侧配置；server 侧激活需要管理员手动操作（防止恶意注册直接上线）

### 信任边界

```
[可信区]                          [半可信区]                    [不可信区]
 lwtop server                     gitee 私有仓库                 公网
 (持有 server 私钥)               (持有 peer 私钥池)             
  │                                │
  │  GITEE_TOKEN 是信任传递的桥梁    │
  └────────────────────────────────┘
                │
                ▼
         可信 client 机器
         (持有自己的 peer 私钥)
```

---

## IP 分配策略

- 网段: `10.0.0.0/24`
- Server: `10.0.0.1`
- Client 范围: `10.0.0.2` ~ `10.0.0.250`
- 保留: `10.0.0.251` ~ `10.0.0.254` (未来扩展)
- 分配方式: pre-gen 时扫描已占用 IP，分配最小可用 IP

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
└── peers/                     # server 公钥 + peer 池（gitee 私有）
    └── SERVER_PUBKEY          # server 公钥文本 (可公开)
    # slot-*.json              # peer 池 (含私钥), 只存 gitee 私有仓库
```

---

## 使用示例

### 管理员：预生成 20 个 peer

```bash
# 在 lwtop 上
sudo gnp-server pregen 20
# 输出: 生成了 slot-01..slot-20, IP 10.0.0.2..10.0.0.21

sudo bash gnp-server pregen 5 --push
# 生成并推送到 gitee
```

### 新机器：一键注册

```bash
# 在 ningsure 上 (有 GITEE_TOKEN)
export GITEE_TOKEN=xxxx
gnp-client register ningsure

# 输出:
# [INFO] 从 gitee 拉取 peer 池...
# [INFO] 选中 peer: ningsure (10.0.0.15/32)
# [INFO] 标记已使用...
# [INFO] 生成配置: ~/.local/share/sing-box/config.json
# [INFO] 安装 sing-box v1.13.16...
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
# [INFO] 激活 peer: ningsure (10.0.0.15/32)
# [INFO] wg set wg0 peer <pubkey> allowed-ips 10.0.0.15/32
# [INFO] ✓ peer ningsure 已激活
```

### 验证

```bash
# 在 client 上
gnp-client test       # 10 秒测试
gnp-client start      # 启动
curl --socks5 127.0.0.1:1080 https://ifconfig.me
# 应返回 lwtop 的公网 IP
```
