#!/usr/bin/env bash
# build-samesource.sh — 在 aipro 上用 openEuler 同源源码编 cfg80211/mac80211（E5 配方）
#
# ⚠️ 唯一正确的构建方式（docs/06）。绝不用 kernel.org 主线源码（struct module 错位）。
# 前提：aipro 可访问 gitee（或从别处传入源码 tar），磁盘 ~10G 空闲。
set -euo pipefail

BRANCH="openEuler-22.03-LTS-SP1"
KREL="5.10.0+"
WORK="${1:-/tmp/oe-kernel-build}"

echo "=== 1/4 取源码（gitee openeuler/kernel @ $BRANCH）==="
if [ ! -d "$WORK/kernel" ] && [ ! -d "$WORK/net" ]; then
  git clone --depth 1 --branch "$BRANCH" https://gitee.com/openeuler/kernel.git "$WORK"
fi
cd "$WORK"

echo "=== 2/4 配置 + vermagic 复现 ==="
zcat /proc/config.gz > .config
echo "+" > .scmversion
make ARCH=arm64 olddefconfig
REL=$(make ARCH=arm64 kernelrelease)
[ "$REL" = "$KREL" ] || { echo "FAIL: kernelrelease=$REL (需 $KREL)"; exit 1; }
make ARCH=arm64 modules_prepare

echo "=== 3/4 编译模块 ==="
make ARCH=arm64 M=net/wireless modules
make ARCH=arm64 M=net/mac80211 modules

echo "=== 4/4 自检：init 重定位必须 0x170（struct module 布局对齐标志）==="
for ko in net/wireless/cfg80211.ko net/mac80211/mac80211.ko; do
  off=$(readelf -rW "$ko" | awk '/rela.gnu.linkonce/{getline; print $1}')
  [ "$off" = "0000000000000170" ] && echo "OK  $ko init@$off" || { echo "BAD $ko init@$off（勿部署！源码血统不对）"; exit 1; }
  modinfo "$ko" | grep vermagic
done

cat <<'EOF'
✓ 构建完成。部署：
  sudo install -m644 net/wireless/cfg80211.ko /lib/modules/5.10.0+/kernel/net/wireless/
  sudo install -m644 net/mac80211/mac80211.ko /lib/modules/5.10.0+/kernel/net/mac80211/
  sudo depmod -a 5.10.0+ && sudo modprobe 8821cu
（或直接用 artifacts/ 里已验证的 .ko + restore-wifi.sh）
EOF
