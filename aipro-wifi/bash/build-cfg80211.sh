#!/usr/bin/env bash
# build-cfg80211.sh — 在 aipro 上补编 cfg80211.ko + mac80211.ko
#
# 背景：OrangePi AIpro 5.10.0+ 内核 CONFIG_CFG80211=m/MAC80211=m，但出厂漏装 .ko。
# 用主线 linux-5.10 + 当前内核 config 编出 vermagic 匹配的模块。
#
# 安全：全程不动 eth0/路由/网络，不加载 8821cu。只在 /tmp 编译 + /lib/modules 写两个 .ko。幂等。
set -euo pipefail

ARCH=arm64
KREL="5.10.0+"
SRC=/tmp/linux-5.10
TARBALL=/tmp/linux-5.10.tar.xz
TUNA=https://mirrors.tuna.tsinghua.edu.cn/kernel/v5.x/linux-5.10.tar.xz

echo "[1/6] 检查内核版本"
[ "$(uname -r)" = "$KREL" ] || echo "WARN: 当前内核 $(uname -r)，非 $KREL"

if [ ! -f "$SRC/net/wireless/cfg80211.ko" ]; then
  echo "[2/6] 下载主线 linux-5.10（tuna）"
  [ -f "$TARBALL" ] || curl -fL --max-time 300 -o "$TARBALL" "$TUNA"
  rm -rf "$SRC"
  tar -xf "$TARBALL" -C /tmp
fi
cd "$SRC"

echo "[3/6] 套 config + 复现 vermagic '+'"
zcat /proc/config.gz > .config
echo "+" > .scmversion
make ARCH="$ARCH" olddefconfig

echo "[4/6] 验证 kernelrelease"
REL=$(make ARCH="$ARCH" kernelrelease)
echo "    kernelrelease = $REL"
[ "$REL" = "$KREL" ] || { echo "FAIL: 不匹配，停止"; exit 1; }

echo "[5/6] 编译"
make ARCH="$ARCH" -j"$(nproc)" modules_prepare
make ARCH="$ARCH" M=net/wireless modules
make ARCH="$ARCH" M=net/mac80211 modules

echo "[6/6] 安装（需 sudo）"
sudo install -D -m644 net/wireless/cfg80211.ko /lib/modules/$KREL/kernel/net/wireless/cfg80211.ko
sudo install -D -m644 net/mac80211/mac80211.ko /lib/modules/$KREL/kernel/net/mac80211/mac80211.ko
sudo depmod -a "$KREL"

echo "✓ 完成。加载："
echo "  sudo insmod /lib/modules/$KREL/kernel/net/wireless/cfg80211.ko"
echo "  sudo insmod /lib/modules/$KREL/kernel/net/mac80211/mac80211.ko"
echo "⚠️  切勿 insmod 8821cu（probe 必 hard lockup，见 docs/02-8821cu-probe-lockup.md）"
