# ns-hub — ningsure 枢纽 WireGuard 星型虚拟网

> 把 **ningsure（唯一公网入站节点）** 做成 WireGuard hub，
> **aipro / coze-pc（只能出站的节点）** 反向拨入，形成 10.99.0.0/24 虚拟局域网。
> 任意成员互访任意端口（ssh/服务/数据库），零逐服务端口映射。

## 拓扑与寻址

```
                    ningsure 10.99.0.1  (hub, wg0 :51820/UDP, 公网 47.103.71.171)
                   /              \
   aipro 10.99.0.2 (家宽NAT)      coze-pc 10.99.0.3 (云主机,端口封死仅出站)
   [ Mac 10.99.0.10 预留 wireguard-go ]
```

- **所有 peer 主动出站**连 hub（NAT/防火墙友好，无需打洞）
- `PersistentKeepalive=25` 维持 NAT 映射
- **AllowedIPs 只放行 10.99.0.0/24**（mesh 网段），绝不接管默认路由——
  遵守 gnp 安全铁律（零断网风险，参照 docs/incident-2026-08-10）

## ⚠️ 部署前提：双层防火墙都要放行 UDP 9100

ningsure 有**两道防火墙**，2026-08-15 实测踩坑：
1. **阿里云控制台**（轻量服务器防火墙）：已有规则 `UDP 9000/9100 0.0.0.0/0` ✓
2. **主机 ufw**：默认 `Policy INPUT DROP` 只放行 22/80/443——**这是实际堵点**
   （tcpdump 能看到包到网卡但 wg/nc 收不到即此症状）。已执行：
   `ufw allow 9100/udp comment ns-hub-wg`（9000 备用口同放）

## ✅ 当前状态（2026-08-15 验收通过）

```
aipro  → hub(10.99.0.1)    13.5ms   握手 ✓
coze-pc → hub               27.9ms   握手 ✓
coze-pc → aipro(10.99.0.2) 41.4ms   ping ✓
```

### ssh 矩阵（全通，旧 7272 反连隧道已退役）

```
aipro/ningsure/coze-pc 三机 ~/.ssh/config 互通条目已部署：
  ningsure → 公网 47.103.71.171（带外原则，不依赖 mesh）
  aipro/coze-pc → wg mesh 10.99.0.x
六向 `ssh <host>` 全部密钥直连 ✓；Mac 的 coze-pc 条目改走
  ProxyJump ningsure → 10.99.0.3（原 127.0.0.1:7272 已删）
coze-pc 的 ssh-tunnel-keepalive.service（ssh -R 7272）已 disable --now
```

> 踩坑：ssh config **首匹配优先**——旧 7272 条目在文件前部，追加的新条目不生效；
> 追加部署前先 grep 清旧块。

## 各节点依赖（2026-08-15 已全部验证就绪）

| 节点 | 内核 wg | 用户态工具 | 状态 |
|---|---|---|---|
| ningsure | 原生已加载 (5.x x86_64) | wg/wg-quick 已装 | ✅ 零准备 |
| coze-pc | 原生 5.15 自带 wireguard.ko | `apt install wireguard-tools` | ✅ 出站 UDP 已实证 |
| aipro | **同源编译 8 模块**（见 artifacts/） | `apt install wireguard-tools` | ✅ `ip link add type wireguard` 实测通过 |
| Mac(预留) | 无需内核（wireguard-go） | brew wireguard-tools | ⏭ 未部署 |

aipro 模块来源：openEuler-22.03-LTS-SP1 同源树编译（配方同 aipro-wifi/docs/06，
`init@0x170` 布局自检通过），产物 `artifacts/aipro-wg-modules.tar.gz`（8 个 .ko，
解包到 `/lib/modules/5.10.0+/kernel/` 对应子目录 + `depmod -a`）。
这推翻了"aipro arm wg 内核支持不好"的旧结论——内核只是没开 CONFIG_WIREGUARD，
dkms 旧版编译失败；同源编译一次通过。

## 目录

```
ns-hub/
├── README.md                        # 本文档（设计+runbook）
├── config/
│   ├── hub.conf.template            # hub wg0 模板（PEERS 变量注入）
│   └── peer.conf.template           # peer wg0 模板
├── scripts/
│   ├── hub-setup.sh                 # ningsure 一键部署 hub
│   └── peer-setup.sh                # aipro/coze-pc 一键部署 peer
└── artifacts/
    └── aipro-wg-modules.tar.gz      # aipro 内核模块自给自足归档
```

## 部署 runbook

```bash
# 1. ningsure（hub）——汇总各 peer 公钥后执行：
sudo bash scripts/hub-setup.sh

# 2. 各 peer（aipro / coze-pc）：
sudo bash scripts/peer-setup.sh <节点名>   # aipro|coze-pc（决定分配的 IP）

# 3. 验收：
ping 10.99.0.1   # peer → hub
ssh lwboy@10.99.0.2   # coze-pc → aipro（原始需求闭环）
```

## 密钥管理

- 每节点一对 Curve25519，**私钥只存节点本地** `/etc/wireguard/<node>.key`（root 600）
- 公钥登记在 hub 配置 + 本 repo `config/inventory.md`（公钥可公开）
- 轮换：节点重新 `peer-setup.sh --regen` → 把新公钥更新进 hub 配置 → 重启两端

## 稳定性设计

- systemd `wg-quick@wg0`（`Restart=on-failure`）+ wg 原生重连
- `PersistentKeepalive=25` < 家宽 NAT UDP 老化时间（~60s+）
- MTU 自动（1420）；验收标准：72h 不断连、断网自愈 <5s

## 安全边界

1. mesh-only 路由（不碰默认路由/无 auto_route 类危险配置）
2. hub 仅做 peer↔peer 转发（iptables FORWARD 限 wg0→wg0），不开 NAT 上网
3. 后续扩展（如 coze-pc 访问 aipro 的 192.168.1.0/24 内网）：在 peer AllowedIPs
   增加网段 + hub 侧放行，默认**不启用**
