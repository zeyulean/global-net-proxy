#!/usr/bin/env bash
# build-ath9k.sh — 补编 ath9k + ath9k_htc + ath 内核模块（主线 in-tree）
#
# 用途：换 ATH9K（AR9271）USB 网卡时，OrangePi 5.10.0+ 内核未开 CONFIG_ATH9K，
#       用此脚本从主线 linux-5.10 源码补编 ath9k_htc.ko（USB AR9271 用）。
#
# 前提：/tmp/linux-5.10 已 modules_prepare（见 build-cfg80211.sh 流程）。
# 安全：纯编译 /tmp，不加载、不碰网络。加载另见 load-ath9k.sh。
set -euo pipefail

ARCH=arm64
SRC=/tmp/linux-5.10

cd "$SRC"

# 1. 开 ath9k 相关 config
./scripts/config --enable CONFIG_ATH_COMMON 2>/dev/null || true
./scripts/config --enable CONFIG_ATH        2>/dev/null || true
./scripts/config --module   CONFIG_ATH9K    2>/dev/null || true
./scripts/config --module   CONFIG_ATH9K_HTC 2>/dev/null || true
make ARCH="$ARCH" olddefconfig 2>&1 | tail -2

echo "=== ath9k config ==="
grep -E "CONFIG_(ATH_COMMON|ATH9K|ATH9K_HTC)=" .config

# 2. vermagic 必须 5.10.0+
echo "+"> .scmversion
REL=$(make ARCH="$ARCH" kernelrelease)
echo "kernelrelease = $REL (需 5.10.0+)"
[ "$REL" = "5.10.0+" ] || { echo "FAIL: vermagic 不匹配"; exit 1; }

# 3. 编译 ath / ath9k
echo "=== 编译 ath (ATH_COMMON) ==="
make ARCH="$ARCH" M=drivers/net/wireless/ath modules 2>&1 | tail -3
echo "=== 编译 ath9k + ath9k_htc ==="
make ARCH="$ARCH" M=drivers/net/wireless/ath/ath9k modules 2>&1 | tail -3

# 4. 产物
echo "=== 产物 ==="
find drivers/net/wireless/ath -name "*.ko" -exec ls -lh {} \;

# 5. 校验 vermagic
echo "=== vermagic 校验 ==="
for ko in drivers/net/wireless/ath/ath.ko \
          drivers/net/wireless/ath/ath9k/ath9k.ko \
          drivers/net/wireless/ath/ath9k/ath9k_htc.ko; do
  [ -f "$ko" ] && echo "$ko: $(modinfo "$ko" | awk -F': ' '/vermagic/{print $2}')"
done

echo ""
echo "✓ 编译完成。加载（需先有 cfg80211+mac80211）："
echo "  sudo insmod /lib/modules/5.10.0+/kernel/net/wireless/cfg80211.ko"
echo "  sudo insmod /lib/modules/5.10.0+/kernel/net/mac80211/mac80211.ko"
echo "  sudo insmod <ath.ko>; sudo insmod <ath9k.ko>; sudo insmod <ath9k_htc.ko>"
