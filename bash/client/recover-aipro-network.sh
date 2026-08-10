#!/usr/bin/env bash
# aipro 断网恢复脚本
# 用途: aipro 被 sing-box tun 破坏路由后, 恢复网络
# 用法: 物理/带外登录 aipro 后, 以 root 运行: bash recover-aipro-network.sh
set -euo pipefail
echo "[1/5] 停止 sing-box 服务 (破坏路由的元凶)"
systemctl stop sing-box-gnp 2>/dev/null || true
systemctl disable sing-box-gnp 2>/dev/null || true

echo "[2/5] 清理 sing-box 添加的策略路由"
ip rule flush 2>/dev/null || true

echo "[3/5] 清理 sing-box 添加的独立路由表"
for t in 2022 100 200; do
  ip route flush table $t 2>/dev/null || true
done

echo "[4/5] 恢复主网卡默认路由"
# 主网卡 eth1, 网关 192.168.0.1 (aipro 局域网)
ip route add default via 192.168.0.1 dev eth1 2>/dev/null || true

echo "[5/5] 清理残留 tun 接口"
ip link del gnp0 2>/dev/null || true
ip link del tun0 2>/dev/null || true

echo ""
echo "=== 验证 ==="
ip route show default
echo "--- 恢复系统 DNS ---"
echo "nameserver 223.5.5.5" > /etc/resolv.conf 2>/dev/null || true
echo "nameserver 119.29.29.29" >> /etc/resolv.conf 2>/dev/null || true
echo ""
echo "✅ 网络应已恢复。若仍不通, 直接重启: reboot"
echo "恢复后 aipro 的 SSH 应可在 192.168.0.108:22 访问"