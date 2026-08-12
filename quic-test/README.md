# GNP QUIC 隧道实验 (hysteria2)

用 **hysteria2 (QUIC)** 替代 UDP-wireguard 的传输方案验证。

## 架构

```
aipro (docker sing-box client)  --hysteria2/QUIC(UDP 443)-->  lwtop (sing-box server)
      mixed 1081                         加密隧道               出口: 8.209.203.17
```

- **client**: aipro 上的 sing-box docker 容器 (ghcr.io/sagernet/sing-box)
- **server**: lwtop 上的 sing-box (1.13.16, with_quic)
- **协议**: hysteria2 (QUIC, TLS 1.3, 多路复用, 抗丢包, 0-RTT)
- **端口**: UDP 443 (伪装 HTTPS, 抗封锁)

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

# ⚠️ 关键: 阿里云 ECS 需放行 UDP 443 (iptables)
iptables -I INPUT -p udp --dport 443 -j ACCEPT
iptables-save > /etc/iptables/rules.v4
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