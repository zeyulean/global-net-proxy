#!/usr/bin/env bash
# run.sh — 在 aipro 上构建并运行 aipro-wifi-router 容器
# 用法（aipro 上，本目录内）：
#   sudo bash run.sh          # 构建镜像 + 交接 wlan1 + 运行
# 前提：wlan1 驱动已加载（bash/aic_load.sh），host gnp 配置在
#       /mnt/disk/lwboy/.local/share/sing-box/config.json（取 hy2 密码）
set -euo pipefail
cd "$(dirname "$0")"

HY2_PASS=$(python3 -c "
import json
c=json.load(open('/mnt/disk/lwboy/.local/share/sing-box/config.json'))
print(next(o['password'] for o in c['outbounds'] if o['type']=='hysteria2'))")

# sing-box 二进制进构建上下文：
#   优先 submodule aipro-wifi/resources（分支 aipro-resources，离线可用）
#   回退 aipro 宿主 gnp 部署路径
if [ -x ./resources/sing-box/sing-box ]; then
  cp -f ./resources/sing-box/sing-box ./sing-box
elif [ -x /mnt/disk/lwboy/.local/share/sing-box/sing-box ]; then
  cp -f /mnt/disk/lwboy/.local/share/sing-box/sing-box ./sing-box
else
  echo "缺少 sing-box 二进制：请先 git submodule update --init ../resources" >&2
  exit 1
fi

echo "=== 1/4 NM 交接 wlan1（持久 unmanaged）==="
nmcli device set wlan1 managed no 2>/dev/null || true
mkdir -p /etc/NetworkManager/conf.d
cat > /etc/NetworkManager/conf.d/unmanage-wlan1.conf <<'EOF'
[keyfile]
unmanaged-devices=interface-name:wlan1
EOF

echo "=== 2/4 构建镜像 ==="
docker build -t aipro-wifi-router:latest .

echo "=== 3/4 运行（host 网络 + privileged）==="
docker rm -f aipro-wifi-router 2>/dev/null || true
docker run -d \
  --name aipro-wifi-router \
  --network host \
  --privileged \
  --restart unless-stopped \
  -e HY2_PASSWORD="$HY2_PASS" \
  -e WIFI_IFACE=wlan1 \
  aipro-wifi-router:latest

echo "=== 4/4 状态 ==="
sleep 6
docker ps --filter name=aipro-wifi-router --format "{{.Names}} {{.Status}}"
docker logs aipro-wifi-router 2>&1 | tail -15
