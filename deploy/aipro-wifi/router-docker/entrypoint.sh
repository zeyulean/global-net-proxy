#!/bin/bash
# entrypoint.sh — aipro-wifi-router 容器入口
# 流程：渲染 sing-box 配置 → 起 sing-box(tproxy) → dnsmasq(DHCP) → hostapd(AP)
# 网络规则：AP 客户端流量 → TPROXY 7893（TCP/UDP）→ hy2/QUIC 出海；DNS → 1053
set -e

IFACE="${WIFI_IFACE:-wlan1}"
AP_IP="${AP_IP:-192.168.88.1}"

echo "[entrypoint] 配置 wlan1 = ${IFACE} (${AP_IP})"
ip link set "$IFACE" up
ip addr flush dev "$IFACE" 2>/dev/null || true
ip addr add "${AP_IP}/24" dev "$IFACE"

echo "[entrypoint] sysctl"
sysctl -w net.ipv4.ip_forward=1

MODE="${ROUTER_MODE:-proxy}"
BAND="${WIFI_BAND:-5g}"
HCONF=/etc/hostapd/hostapd.conf
[ "$BAND" = "24g" ] && HCONF=/etc/hostapd/hostapd-24g.conf
echo "[entrypoint] 频段: $BAND ($HCONF)"
echo "[entrypoint] 模式: $MODE"

if [ "$MODE" = "direct" ]; then
    # ── 原通道模式 (gnpoff): NAT 直连, 不起 sing-box/tproxy ──
    WAN_IF=$(ip route get 223.5.5.5 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | head -1)
    [ -z "$WAN_IF" ] && WAN_IF=eth0
    echo "[entrypoint] direct: NAT 192.168.88.0/24 → $WAN_IF"
    iptables -t nat -C POSTROUTING -s 192.168.88.0/24 -o "$WAN_IF" -j MASQUERADE 2>/dev/null ||     iptables -t nat -A POSTROUTING -s 192.168.88.0/24 -o "$WAN_IF" -j MASQUERADE
    cleanup() {
        iptables -t nat -D POSTROUTING -s 192.168.88.0/24 -o "$WAN_IF" -j MASQUERADE 2>/dev/null || true
        kill $(jobs -p) 2>/dev/null || true
        exit 0
    }
    trap cleanup SIGTERM SIGINT
    echo "[entrypoint] direct: dnsmasq (DHCP + DNS 223.5.5.5)"
    dnsmasq --conf-file=/etc/dnsmasq/dnsmasq.conf --port=53 --server=223.5.5.5 --keep-in-foreground &
    sleep 1
    ip link set "$IFACE" down 2>/dev/null || true
    echo "[entrypoint] direct: hostapd"
    exec hostapd $HCONF
fi

# ── 代理模式 (gnpon, 默认) ──
echo "[entrypoint] 渲染 sing-box 配置"
sed "s/__HY2_PASSWORD__/${HY2_PASSWORD:?need HY2_PASSWORD env}/" \
    /etc/sing-box/config.template > /etc/sing-box/config.json

echo "[entrypoint] tproxy 路由规则"
ip route replace local default dev lo table 100
ip rule del fwmark 1 table 100 2>/dev/null || true
ip rule add fwmark 1 table 100

echo "[entrypoint] iptables mangle 规则"
iptables -t mangle -N AIPRO_TPROXY 2>/dev/null || true
iptables -t mangle -F AIPRO_TPROXY
# DNS 最优先劫持（包括发往 AP 自身 192.168.88.1:53 的查询，必须先于私网 RETURN）
iptables -t mangle -A AIPRO_TPROXY -p udp --dport 53 -j TPROXY --on-port 7893 --tproxy-mark 1
# 私有网段直连（本地管理/局域网访问不受代理影响）
iptables -t mangle -A AIPRO_TPROXY -d 0.0.0.0/8 -j RETURN
iptables -t mangle -A AIPRO_TPROXY -d 10.0.0.0/8 -j RETURN
iptables -t mangle -A AIPRO_TPROXY -d 127.0.0.0/8 -j RETURN
iptables -t mangle -A AIPRO_TPROXY -d 169.254.0.0/16 -j RETURN
iptables -t mangle -A AIPRO_TPROXY -d 172.16.0.0/12 -j RETURN
iptables -t mangle -A AIPRO_TPROXY -d 192.168.0.0/16 -j RETURN
iptables -t mangle -A AIPRO_TPROXY -d 224.0.0.0/4 -j RETURN
iptables -t mangle -A AIPRO_TPROXY -d 240.0.0.0/4 -j RETURN
# DNS 与其余 TCP/UDP 统一进 tproxy，由 sing-box hijack-dns 规则应答
iptables -t mangle -A AIPRO_TPROXY -p tcp -j TPROXY --on-port 7893 --tproxy-mark 1
iptables -t mangle -A AIPRO_TPROXY -p udp -j TPROXY --on-port 7893 --tproxy-mark 1
iptables -t mangle -C PREROUTING -i "$IFACE" -j AIPRO_TPROXY 2>/dev/null || \
iptables -t mangle -A PREROUTING -i "$IFACE" -j AIPRO_TPROXY

echo "[entrypoint] 清理函数"
cleanup() {
    echo "[entrypoint] 清理规则"
    iptables -t mangle -D PREROUTING -i "$IFACE" -j AIPRO_TPROXY 2>/dev/null || true
    iptables -t mangle -F AIPRO_TPROXY 2>/dev/null || true
    ip rule del fwmark 1 table 100 2>/dev/null || true
    kill $(jobs -p) 2>/dev/null || true
    exit 0
}
trap cleanup SIGTERM SIGINT

echo "[entrypoint] 启动 sing-box (tproxy → hy2/QUIC)"
/usr/local/bin/sing-box run -c /etc/sing-box/config.json &
sleep 2

echo "[entrypoint] 启动 dnsmasq (DHCP)"
dnsmasq --conf-file=/etc/dnsmasq/dnsmasq.conf --keep-in-foreground &
sleep 1

ip link set "$IFACE" down 2>/dev/null || true
echo "[entrypoint] 启动 hostapd ($HCONF)"
exec hostapd $HCONF
