#!/usr/bin/env bash
# peer-setup.sh — ns-hub 节点端一键部署（aipro / coze-pc / 其他出站节点）
# 用法（节点上，root）：bash peer-setup.sh <节点名> [--regen]
#   节点名 → 固定 IP：aipro=10.99.0.2  coze-pc=10.99.0.3  其他见 README
# 环境变量：HUB_PUB（hub 公钥；不传则提示手工填）
set -euo pipefail

NODE="${1:?用法: peer-setup.sh <节点名> [--regen]}"
REGEN=0; [ "${2:-}" = "--regen" ] && REGEN=1

case "$NODE" in
  aipro)   IP=10.99.0.2 ;;
  coze-pc) IP=10.99.0.3 ;;
  *) echo "未知节点 $NODE（aipro|coze-pc）"; exit 1 ;;
esac

HUB_ENDPOINT="${HUB_ENDPOINT:-47.103.71.171:51820}"
WG_DIR=/etc/wireguard
KEY=$WG_DIR/$NODE.key

mkdir -p "$WG_DIR"; cd "$WG_DIR"

# 1. 密钥
if [ ! -f "$KEY" ] || [ "$REGEN" = 1 ]; then
  umask 077; wg genkey > "$KEY"
  echo "[$NODE] 私钥已生成"
fi
PUB=$(wg pubkey < "$KEY")
echo "[$NODE] 公钥: $PUB   ← 把这一行交给 hub (hub-setup 的 [Peer])"

# 2. hub 公钥
if [ -z "${HUB_PUB:-}" ]; then
  read -rp "粘贴 hub 公钥: " HUB_PUB
fi

# 3. 配置（mesh-only 路由，绝不接管默认路由）
cat > $WG_DIR/wg0.conf <<EOF
# ns-hub peer:$NODE — 由 peer-setup.sh 生成
[Interface]
PrivateKey = $(cat "$KEY")
Address = ${IP}/24

[Peer]
PublicKey = ${HUB_PUB}
Endpoint = ${HUB_ENDPOINT}
AllowedIPs = 10.99.0.0/24
PersistentKeepalive = 25
EOF
chmod 600 $WG_DIR/wg0.conf

# 4. aipro：确保内核模块开机可载（rtnl 自动加载 + 显式双保险）
if [ "$NODE" = aipro ]; then
  modprobe wireguard 2>/dev/null || true
  echo wireguard > /etc/modules-load.d/ns-hub-wireguard.conf
fi

# 5. 起服务
systemctl enable --now wg-quick@wg0 2>/dev/null || systemctl restart wg-quick@wg0
sleep 1
ip -br addr show wg0
echo "验收: ping 10.99.0.1 （hub 安全组放行 UDP 51820 后即通）"
