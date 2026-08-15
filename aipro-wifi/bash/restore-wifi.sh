#!/usr/bin/env bash
# restore-wifi.sh — aipro WiFi 快速恢复（从本 repo 的 artifacts/ 部署，无需编译）
#
# 适用：模块丢失/损坏/系统重刷后恢复 WiFi。
# 前提：aipro 内核仍为 5.10.0+（OrangePi AIpro 20T 出厂内核）。
# 用法（在 global-net-proxy repo 根目录）：
#   ./aipro-wifi/bash/restore-wifi.sh [user@host]     # 默认 aipro
set -euo pipefail

HOST="${1:-aipro}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
ART="$HERE/artifacts"
KREL="5.10.0+"

# 1. 校验本地产物
for f in cfg80211.ko mac80211.ko 8821cu.ko; do
  [ -f "$ART/$f" ] || { echo "缺少 $ART/$f"; exit 1; }
done

echo "=== 1/5 传输模块到 $HOST ==="
scp "$ART/cfg80211.ko" "$HOST:/tmp/"
scp "$ART/mac80211.ko" "$HOST:/tmp/"
scp "$ART/8821cu.ko" "$HOST:/tmp/"

echo "=== 2/5 部署 + 校验 md5 ==="
ssh "$HOST" 'sudo -S sh -s' <<'EOF'
set -e
KREL=5.10.0+
# 备份现存的（若与目标不同）
for p in "kernel/net/wireless/cfg80211.ko" "kernel/net/mac80211/mac80211.ko" "8821cu.ko"; do
  f="/lib/modules/$KREL/$p"
  [ -f "$f" ] && cp -a "$f" "$f.bak.$(date +%s)" || true
done
install -m644 /tmp/cfg80211.ko "/lib/modules/$KREL/kernel/net/wireless/cfg80211.ko"
install -m644 /tmp/mac80211.ko "/lib/modules/$KREL/kernel/net/mac80211/mac80211.ko"
install -m644 /tmp/8821cu.ko   "/lib/modules/$KREL/8821cu.ko"
md5sum "/lib/modules/$KREL/kernel/net/wireless/cfg80211.ko" \
       "/lib/modules/$KREL/kernel/net/mac80211/mac80211.ko" \
       "/lib/modules/$KREL/8821cu.ko"
EOF
cat <<'NOTE'
期望 md5：
  b3ce310d67ba6e873e447fa357f93b3a  cfg80211.ko
  0b77c58831d6c8403814e2bac3aef48f  mac80211.ko
  2168a29a3c75589dd5a0a4b845b2a736  8821cu.ko
NOTE

echo "=== 3/5 rfkill 陷阱清除 + depmod + 自启配置 ==="
ssh "$HOST" 'sudo -S sh -c "
  [ -f /lib/modules/'$KREL'/kernel/net/rfkill/rfkill.ko ] && mv /lib/modules/'$KREL'/kernel/net/rfkill/rfkill.ko{,.stale-bak} || true
  depmod -a '$KREL' &&
  printf \"cfg80211\nmac80211\n8821cu\n\" > /etc/modules-load.d/aipro-wifi.conf &&
  echo CONFIG-OK"'

echo "=== 4/5 加载（若未加载）==="
ssh "$HOST" 'sudo -S sh -c "
  sysctl -w kernel.panic_on_oops=0 >/dev/null
  lsmod | grep -q \"^cfg80211\" || modprobe cfg80211
  lsmod | grep -q \"^mac80211\" || modprobe mac80211
  lsmod | grep -q \"^8821cu\"   || modprobe 8821cu
  lsmod | grep -E \"^(cfg80211|mac80211|8821cu)\""'

echo "=== 5/5 验证 ==="
sleep 6
ssh "$HOST" 'ip -br addr show wlan0 && iw dev wlan0 link | head -3' || \
  echo "wlan0 未连接——若 NM 未自动连：sudo nmcli connection up XinYuan"
echo "✓ 恢复流程完成。故障排查手册：aipro-wifi/docs/06-struct-module-mismatch.md"
