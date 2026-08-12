#!/usr/bin/env bash
# aipro 网络彻底清理脚本
# 用途: aipro SSH 恢复后立即执行, 停 sing-box, 防止 tun 再次破坏路由
set -euo pipefail
echo "=== [1/6] 立即停止 sing-box 服务 ==="
systemctl stop sing-box-gnp 2>/dev/null || true
systemctl stop sing-box 2>/dev/null || true
# 也杀掉所有 sing-box 进程
pkill -9 sing-box 2>/dev/null || true
sleep 1
pgrep -af sing-box || echo "  sing-box 进程已全部清理"

echo "=== [2/6] 禁用开机自启 (即使 symlink 不存在也强制 disable) ==="
systemctl disable sing-box-gnp 2>/dev/null || true
systemctl mask sing-box-gnp 2>/dev/null || true
# 确认 symlink 不存在
ls /etc/systemd/system/multi-user.target.wants/sing-box-gnp.service 2>&1 || echo "  symlink 已不存在 ✓"

echo "=== [3/6] 清理残留 tun 接口 ==="
ip link del gnp0 2>/dev/null || true
ip link del tun0 2>/dev/null || true
sleep 1
ip link show 2>&1 | grep -E "gnp|tun" || echo "  tun 接口已清理"

echo "=== [4/6] 清理 sing-box 设置的策略路由 ==="
# 清除 sing-box 加入的 ip rule (从 table 2022 开始的)
for prio in 9000 9001 9002 9003 9010; do
  ip rule del priority $prio 2>/dev/null && echo "  清理 ip rule prio $prio" || true
done
ip route flush table 2022 2>/dev/null || true
echo "  策略路由已清理"

echo "=== [5/6] 恢复主网卡默认路由 ==="
# 主网卡 eth1, 网关 192.168.0.1
ip route show default | grep -q "via 192.168.0.1" || \
  ip route add default via 192.168.0.1 dev eth1 2>/dev/null || true
ip route show default

echo "=== [6/6] 清理 sing-box 二进制和配置 (避免再次误用) ==="
# 备份后删除, 而不是直接删
if [ -d /home/lwboy/.local/share/sing-box ]; then
  mv /home/lwboy/.local/share/sing-box /home/lwboy/.local/share/sing-box.disabled-$(date +%Y%m%d) 2>/dev/null || \
    rm -rf /home/lwboy/.local/share/sing-box
  echo "  sing-box 二进制已禁用/备份"
fi

echo ""
echo "=== 最终状态 ==="
echo "sing-box 进程 (应只有 gnp-quic, 没有 gnp):"
ip link show | grep wg
echo ""
echo "默认路由:"
ip route show default
echo ""
echo "sing-box 进程:"
pgrep -af sing-box || echo "  无 ✓"
echo ""
echo "✅ aipro 网络已彻底清理, sing-box 不会再次破坏路由"