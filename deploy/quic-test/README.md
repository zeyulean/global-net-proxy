# GNP QUIC 隧道实验 (hysteria2)

用 **hysteria2 (QUIC)** 替代 UDP-wireguard 的传输方案验证。

## 架构

```
aipro (docker sing-box client)  --hysteria2/QUIC(UDP 5766)-->  lwtop (sing-box server)
      mixed 1081                         加密隧道               出口: 8.209.203.17
```

- **client**: aipro 上的 sing-box docker 容器 (ghcr.io/sagernet/sing-box)
- **server**: lwtop 上的 sing-box (1.13.16, with_quic)
- **协议**: hysteria2 (QUIC, TLS 1.3, 多路复用, 抗丢包, 0-RTT)
- **端口**: UDP 5766 (2026-10-05 从 443 迁出, 让位 TCP 443 给未来 vless/reality 并脱离 443 双栈歧义)

## 部署步骤

> ⚠️ 2026-10-05 起服务端已迁到 v2 布局（plan §4.3）：`/opt/gnp-quic` → `/opt/gnp`，
> `gnp-hy2` → `gnps.service`，配置由 `config.toml` 渲染。**不要再手工写 unit/证书**，
> 一条命令搞定（旧布局已归档在 `/opt/gnp/backups/gnp-quic-legacy`）。

### 1. lwtop (server)

```bash
# 事实源: repo 的 deploy/hosts/lwtop-server.toml (8 个用户, 端口 5766, salamander obfs)
scp deploy/hosts/lwtop-server.toml lwtop:/tmp/
ssh lwtop 'sudo mkdir -p /opt/gnp/{bin,etc/tick.d,var,secrets,backups}
           sudo install -m 600 /tmp/lwtop-server.toml /opt/gnp/config.toml
           sudo /opt/gnp/bin/gnps install --config /opt/gnp/config.toml'

# gnps install 会: 渲染 config.json → sing-box check → 写 gnps.service → 装 tick 调度
#                 → enable --now gnps → disable 旧 gnp-hy2 → 归档 /opt/gnp-quic
# ⚠️ 关键: 防火墙放行 UDP 5766 —— 必须走 ufw (持久化), 不要裸 iptables
ssh lwtop 'sudo ufw allow 5766/udp comment "gnp hy2 QUIC"; sudo ufw status | grep 5766'
```

验证：`ssh lwtop 'sudo /opt/gnp/bin/gnps status'`（gnps active + UDP 5766 在听），
客户端侧 `gnpc probe --skip-mtu` 走真实路径握手。

### 2. aipro (docker client)

```bash
mkdir -p ~/gnp-quic-test
# 配置见 aipro-docker-client.json
docker run -d --name gnp-quic-client --restart unless-stopped \
  -v ~/gnp-quic-test:/etc/sing-box \
  -p 1081:1081 \
  ghcr.io/sagernet/sing-box:latest run -c /etc/sing-box/config.json
```

### 3. 验证

```bash
curl -x http://127.0.0.1:1081 -s -o /dev/null -w '%{http_code}\n' https://github.com  # 200
curl -x http://127.0.0.1:1081 -s https://ifconfig.me  # 8.209.203.17 (lwtop 出口)
```

## 踩坑记录

1. **iptables 未放行 UDP 443** —— 阿里云 ECS 内置防火墙默认不放行 443 UDP，导致 server 收到包但不回。加 `iptables -I INPUT -p udp --dport 443 -j ACCEPT` 后解决。
2. **特权端口** —— 443 < 1024，sing-box 需 root 运行（systemd `User=root`）。
3. **端口冲突** —— aipro 现有 gnp 服务占 1080，容器用 1081。
4. **⚠️ 2026-10-05 事故：重启后 hy2 断网两天（真根因）** —— 原部署用裸 iptables 放行
   443/udp，lwtop ufw 又是 enabled。重启后裸规则丢失、ufw 默认 deny 接管 → 所有新
   hy2 握手无声被丢（长活流靠 conntrack ESTABLISHED 绕过默认 deny，重启清空 conntrack
   后缺口暴露）。表象酷似"MTU 黑洞"（内核计数器扫描的松窗口差分被全机 UDP 噪声污染，
   造出假的尺寸截止点）。**修复与铁律：hy2 端口一律 `ufw allow <port>/udp` 持久化；排查"无声超时"
   先查服务端防火墙协议+端口双栈放行，尺寸扫描结论必须过单调性校验**（`gnp-client probe` 已内置）。
   当日已迁 5766/udp（云安全组 + ufw 双侧放行），443/udp 两侧均已关闭。
5. **服务端换进程后客户端长跑实例会僵死** —— 服务端迁到 `gnps` 后，Mac 侧
   `clash_api` delay 仍报 113ms「健康」，真实流量却全丢（`status --brief` 的 `exit=-`）。
   **服务端迁移/重启后必须逐台重启客户端 sing-box**（`gnpc stop && gnpc start`），
   guard 看不见这种「假健康」。已记入 plan §7.3 #19。