#!/usr/bin/env bash
# hub-setup.sh — ningsure WireGuard hub 一键部署
# 用法（ningsure 上，root）：bash hub-setup.sh
# 前提：peers/ 下的公钥文件已就位（peer-setup.sh 在各节点生成并回传）
set -euo pipefail

WG_DIR=/etc/wireguard
HUB_KEY=$WG_DIR/ningsure.key
HUB_PORT=51820
HUB_IP=10.99.0.1

mkdir -p "$WG_DIR"
cd "$WG_DIR"

# 1. hub 密钥（幂等：已存在则复用）
if [ ! -f "$HUB_KEY" ]; then
  umask 077
  wg genkey > "$HUB_KEY"
  echo "[hub] 新私钥已生成（妥善备份，丢失则全网重配）"
fi
HUB_PUB=$(wg pubkey < "$HUB_KEY")

# 2. 渲染配置（peers 公钥来自 inventory 段，由部署时注入）
read -r -d '' PEERS <<'EOF' || true
EOF
if [ -z "$PEERS" ]; then
  echo "[hub] 未提供 peers —— 生成仅骨架，后续手工补 [Peer] 段"
fi

cat > $WG_DIR/wg0.conf <<EOF
# ns-hub (ningsure) — 由 hub-setup.sh 生成
[Interface]
PrivateKey = $(cat "$HUB_KEY")
Address = ${HUB_IP}/24
ListenPort = ${HUB_PORT}
# peer 间转发（仅 wg0 内部，不开 NAT）
PostUp   = sysctl -w net.ipv4.ip_forward=1; iptables -A FORWARD -i %i -o %i -j ACCEPT
PostDown = iptables -D FORWARD -i %i -o %i -j ACCEPT 2>/dev/null || true
${PEERS}
EOF
chmod 600 $WG_DIR/wg0.conf

# 3. 起服务
systemctl enable --now wg-quick@wg0 2>/dev/null || systemctl restart wg-quick@wg0
sleep 1

echo "== hub 就绪 =="
echo "  hub 公钥（分发给 peers）: $HUB_PUB"
echo "  监听: 0.0.0.0:${HUB_PORT}/UDP（记得云安全组放行）"
ip -br addr show wg0
wg show wg0 || true
