#!/usr/bin/env bash
# safe-load-cfg80211.sh — 安全加载 cfg80211/mac80211（绕 rfkill.ko 残留陷阱）
#
# modprobe cfg80211 会拉 /lib/modules/.../rfkill/rfkill.ko，
# 与内核 built-in 的 rfkill(=y) 冲突：
#   "rfkill: exports duplicate symbol rfkill_alloc (owned by kernel)"
# 故必须用 insmod 绕过依赖解析。
#
# 安全：cfg80211/mac80211 只注册 genl/mac80211 子系统，不接管网卡，不动路由。
# 失败仅报 modprobe 错，不伤 ssh。
set -euo pipefail

KREL="5.10.0+"
CFG=/lib/modules/$KREL/kernel/net/wireless/cfg80211.ko
MAC=/lib/modules/$KREL/kernel/net/mac80211/mac80211.ko

echo "=== 校验 vermagic（必须 5.10.0+ SMP mod_unload aarch64）==="
for ko in "$CFG" "$MAC"; do
  vm=$(modinfo "$ko" | awk -F': ' '/vermagic/{print $2}')
  echo "$ko : $vm"
  [[ "$vm" == "5.10.0+ SMP mod_unload aarch64" ]] || { echo "FAIL: vermagic 不匹配，停止"; exit 1; }
done

if lsmod | grep -q cfg80211; then
  echo "cfg80211 已加载，跳过"
else
  echo "=== insmod cfg80211 ==="
  sudo insmod "$CFG"
fi
if lsmod | grep -q mac80211; then
  echo "mac80211 已加载，跳过"
else
  echo "=== insmod mac80211 ==="
  sudo insmod "$MAC"
fi

echo "=== 结果 ==="
lsmod | grep -E 'cfg80211|mac80211'
echo "✓ 完成。SSH 不受影响。"
