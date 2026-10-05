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

### 1. lwtop (server)

```bash
# 证书
mkdir -p /opt/gnp-quic/certs
openssl req -x509 -nodes -newkey rsa:2048 -keyout server.key -out server.crt \
  -days 3650 -subj "/CN=gnp-quic"

# 配置 (见 lwtop-hy2-server.json)
# systemd 服务
cat > /etc/systemd/system/gnp-hy2.service << 'EOF'
[Unit]
Description=GNP Hysteria2 QUIC Server
After=network.target

[Service]
Type=simple
ExecStart=/home/lw/.local/share/sing-box/sing-box run -c /opt/gnp-quic/config.json
Restart=always
RestartSec=3
User=root

[Install]
WantedBy=multi-user.target
EOF
systemctl enable --now gnp-hy2

# ⚠️ 关键: 防火墙放行 UDP 5766 —— 必须走 ufw (持久化), 不要裸 iptables
ufw allow 5766/udp comment 'gnp hy2 QUIC'
ufw status | grep 5766
```

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