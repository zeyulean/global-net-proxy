#!/usr/bin/env bash
# aiprowifi.sh — aipro WiFi 控制（start/stop/status）
#
# ✅ 2026-08-15 修复成功（docs/06）：
#   wlan0 已可用：原厂 8821cu（c820 实为 8821CU 硅片，chip_id=0x09 非错读）
#   + openEuler-22.03-LTS-SP1 同源编的 cfg80211/mac80211（ABI 对齐，init 正常执行）
#   加载序列：cfg80211 → mac80211 → 8821cu（顺序不可乱，全部 insmod 直装）
#
# 用法:
#   ./aiprowifi.sh start    # 加载三模块（NM 自动按 profile 连 WiFi）
#   ./aiprowifi.sh stop     # 卸载
#   ./aiprowifi.sh status   # 查看状态
set -euo pipefail

# ===== 配置 =====
WIFI_IFACE="wlan0"
AP_SUBNET="192.168.88.0/24"     # AP 子网（避开 eth0 192.168.1.0/24；AP 模式待做）
GNP_PROXY="192.168.1.2:1080"    # aipro 上 gnp sing-box mixed 代理（出口走 hy2 海外）

# ===== 模块路径 =====
KREL="5.10.0+"
CFG80211=/lib/modules/$KREL/kernel/net/wireless/cfg80211.ko
MAC80211=/lib/modules/$KREL/kernel/net/mac80211/mac80211.ko
DRV_8821CU=/lib/modules/$KREL/8821cu.ko

load_modules() {
  lsmod | grep -q "^cfg80211" || sudo insmod "$CFG80211"
  lsmod | grep -q "^mac80211" || sudo insmod "$MAC80211"
  lsmod | grep -q "^8821cu"   || sudo insmod "$DRV_8821CU"
  lsmod | grep -E "^cfg80211|^mac80211|^8821cu"
}

unload_modules() {
  lsmod | grep -q "^8821cu"   && sudo rmmod 8821cu   || true
  lsmod | grep -q "^mac80211" && sudo rmmod mac80211 || true
  lsmod | grep -q "^cfg80211" && sudo rmmod cfg80211 || true
}

case "${1:-status}" in
  start)
    echo "=== 加载无线栈（cfg80211 → mac80211 → 8821cu）==="
    sudo sysctl -w kernel.panic_on_oops=0 >/dev/null || true
    load_modules
    sleep 3
    ip -br addr show "$WIFI_IFACE" 2>/dev/null || echo "wlan0 未出现"
    ;;
  stop)
    unload_modules
    ;;
  status)
    echo "=== 无线模块 ==="
    lsmod | grep -E "^cfg80211|^mac80211|^8821cu" || echo "(无)"
    echo "=== wlan 网卡 ==="
    ip -br addr | grep -E "wlan|wlx" || echo "(无 wlan)"
    echo "=== 连接状态 ==="
    iw dev "$WIFI_IFACE" link 2>/dev/null | head -4 || true
    ;;
  *)
    echo "用法: $0 {start|stop|status}"
    exit 1
    ;;
esac
