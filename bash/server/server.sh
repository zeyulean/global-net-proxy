#!/bin/bash
#===============================================================
# global-net-proxy — WireGuard Server 一键脚本 (Ubuntu)
#
# 架构: sing-box client(tun + wg endpoint) → 本 wg server → NAT 出网
# 本机只做: wg server + 内核转发 + SNAT, 不装 sing-box
#
# 用法:
#   sudo bash server.sh                # 交互式安装
#   sudo bash server.sh --install      # 安装
#   sudo bash server.sh --uninstall    # 卸载
#   sudo bash server.sh --peers        # 查看所有客户端
#   sudo bash server.sh --add-peer     # 添加一个客户端(生成配置)
#   sudo bash server.sh --status       # 查看状态
#   sudo bash server.sh --help, -h     # 帮助
#
# 支持: Ubuntu 20.04+ / Debian 11+ (仅限 Linux server)
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

WG_CONF="/etc/wireguard/wg0.conf"
WG_IFACE="wg0"
WG_DIR="/etc/wireguard"
# 内网网段 (客户端 sing-box tun 网段, 与 server 不冲突即可)
WG_SUBNET="10.99.0.0/24"
SERVER_IP="10.99.0.1/24"
# 默认监听端口
WG_PORT="${WG_PORT:-51820}"

#---------------------------------------------------------------
ACTION="${1:-install}"
case "$ACTION" in
    --install)   ACTION="install" ;;
    --uninstall) ACTION="uninstall" ;;
    --peers)     ACTION="peers" ;;
    --add-peer)  ACTION="add-peer" ;;
    --status)    ACTION="status" ;;
    --help|-h)   ACTION="help" ;;
    *)           ACTION="help" ;;
esac

show_help() {
    echo "global-net-proxy — WireGuard Server (Ubuntu)"
    echo ""
    echo "用法: sudo bash $0 [选项]"
    echo "  (无参数)        交互式安装 (生成密钥/配置/客户端)"
    echo "  --install       同上"
    echo "  --uninstall     卸载 (停止服务/删配置/移除包)"
    echo "  --peers         列出所有已注册客户端"
    echo "  --add-peer      <名称>  添加新客户端并输出其配置"
    echo "  --status        查看状态"
    echo "  --help, -h      帮助"
    echo ""
    echo "环境变量: WG_PORT=端口 (默认51820)"
    exit 0
}

check_root() { [[ $EUID -ne 0 ]] && error "请用 root 运行: sudo bash $0"; }

detect_os() {
    if [[ -f /etc/os-release ]]; then
        . /etc/os-release
        [[ "$ID" != "ubuntu" && "$ID" != "debian" ]] && error "仅支持 Ubuntu/Debian (当前: $ID)"
    else
        error "无法检测操作系统"
    fi
    info "检测到系统: $ID $VERSION_ID"
}

# 检测主出口网卡 (用于 NAT)
detect_wan_iface() {
    local iface
    iface=$(ip route show default | awk '/default/ {print $5; exit}')
    [[ -z "$iface" ]] && error "未找到默认出口网卡"
    echo "$iface"
}

#---------------------------------------------------------------
# 安装 wireguard 工具
install_wg() {
    if command -v wg &>/dev/null; then
        info "wireguard-tools 已安装"
        return
    fi
    info "安装 wireguard-tools..."
    apt-get update -qq
    apt-get install -y -qq wireguard-tools
}

#---------------------------------------------------------------
# 生成密钥(若不存在)
ensure_keys() {
    if [[ ! -f "$WG_DIR/server.key" ]]; then
        wg genkey | tee "$WG_DIR/server.key" >/dev/null
        chmod 600 "$WG_DIR/server.key"
    fi
    if [[ ! -f "$WG_DIR/server.pub" ]]; then
        wg pubkey < "$WG_DIR/server.key" > "$WG_DIR/server.pub"
    fi
    SERVER_PRIV=$(cat "$WG_DIR/server.key")
}

#---------------------------------------------------------------
# 安装核心逻辑
do_install() {
    check_root
    detect_os
    install_wg
    ensure_keys

    # 若已有配置, 询问是否重建
    if [[ -f "$WG_CONF" ]]; then
        read -rp "检测到已有 $WG_CONF, 覆盖重建? [y/N] " yn
        [[ "$yn" != "y" && "$yn" != "Y" ]] && { info "已取消"; exit 0; }
    fi

    local wan
    wan=$(detect_wan_iface)
    info "出口网卡: $wan"

    # 生成 server 配置
    cat > "$WG_CONF" <<EOF
[Interface]
Address = $SERVER_IP
ListenPort = $WG_PORT
PrivateKey = $SERVER_PRIV
# 允许转发进来的流量 (客户端可访问的内部网段, 按需添加)
PostUp = iptables -A FORWARD -i $WG_IFACE -j ACCEPT; iptables -A FORWARD -o $WG_IFACE -j ACCEPT; iptables -t nat -A POSTROUTING -o $wan -j MASQUERADE
PostDown = iptables -D FORWARD -i $WG_IFACE -j ACCEPT; iptables -D FORWARD -o $WG_IFACE -j ACCEPT; iptables -t nat -D POSTROUTING -o $wan -j MASQUERADE
EOF
    chmod 600 "$WG_CONF"

    # 开启内核转发
    sysctl -w net.ipv4.ip_forward=1 >/dev/null
    grep -q '^net.ipv4.ip_forward' /etc/sysctl.conf || echo 'net.ipv4.ip_forward=1' >> /etc/sysctl.conf

    # 启动服务
    systemctl enable wg-quick@wg0 >/dev/null 2>&1 || true
    systemctl restart wg-quick@wg0 2>/dev/null || wg-quick up wg0

    info "WireGuard Server 安装完成!"
    info "  内网地址: $SERVER_IP"
    info "  监听端口: $WG_PORT"
    info "  公钥: $(cat $WG_DIR/server.pub)"
    info ""
    info "添加客户端: sudo bash $0 --add-peer <名称>"
}

#---------------------------------------------------------------
# 添加客户端
do_add_peer() {
    check_root
    [[ -f "$WG_CONF" ]] || error "Server 未安装, 先运行安装"
    local name="${1:-client-$(date +%s)}"

    local client_priv client_pub
    client_priv=$(wg genkey)
    client_pub=$(wg pubkey <<< "$client_priv")

    # 分配下一个内网 IP (从 10.99.0.2 开始递增)
    local idx=2
    while grep -q "10.99.0.$idx/32" "$WG_CONF"; do idx=$((idx+1)); done
    local next_ip="10.99.0.$idx/32"

    # 追加 peer 到 server 配置
    cat >> "$WG_CONF" <<EOF

[Peer]
# $name
PublicKey = $client_pub
AllowedIPs = $next_ip
EOF
    systemctl restart wg-quick@wg0 2>/dev/null || wg-quick down wg0 && wg-quick up wg0

    # 生成客户端配置 (供 sing-box client 参考)
    local server_pub server_host
    server_pub=$(cat "$WG_DIR/server.pub")
    server_host=$(curl -s --max-time 5 ifconfig.me 2>/dev/null || hostname -I | awk '{print $1}')

    local client_conf="/etc/wireguard/client-$name.conf"
    cat > "$client_conf" <<EOF
# global-net-proxy 客户端配置 ($name)
# 注意: 这是 sing-box client 的 peer 参考, 实际由 sing-box 的
# endpoint 配置使用 (见 docs/client-singbox.md)
[Interface]
PrivateKey = $client_priv
Address = ${next_ip}

[Peer]
PublicKey = $server_pub
Endpoint = $server_host:$WG_PORT
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
EOF
    chmod 600 "$client_conf"

    info "客户端 [$name] 添加成功!"
    echo ""
    echo "======== 客户端配置已保存: $client_conf ========"
    echo " (内含私钥, 请安全传输到客户端)"
    echo ""
    echo "--- sing-box client 需要的参数 ---"
    echo "  私钥(private_key): $client_priv"
    echo "  地址(address): ${next_ip}"
    echo "  server(对端公网): $server_host:$WG_PORT"
    echo "  对端公钥(server_public_key): $server_pub"
    echo "  allowed_ips: 0.0.0.0/0, ::/0"
}

#---------------------------------------------------------------
do_peers() {
    check_root
    [[ -f "$WG_CONF" ]] || error "Server 未安装"
    echo -e "${CYAN}===== 已注册客户端 =====${NC}"
    grep -E '^\s*(#|PublicKey|AllowedIPs)' "$WG_CONF" | paste - - - 2>/dev/null || \
        awk '/^\[Peer\]/{n++; print "--- Peer " n " ---"} /^#/{print $0} /PublicKey/{print "  " $0} /AllowedIPs/{print "  " $0}' "$WG_CONF"
}

do_status() {
    check_root
    echo -e "${CYAN}===== WireGuard Server 状态 =====${NC}"
    if ! command -v wg &>/dev/null; then echo "WG 未安装"; return; fi
    [[ -f "$WG_CONF" ]] && echo "配置: $WG_CONF ✓" || echo "配置: 不存在"
    if ip link show "$WG_IFACE" &>/dev/null; then
        echo "接口 $WG_IFACE: 已启动 ✓"
        wg show "$WG_IFACE" 2>/dev/null || echo "(无详情)"
    else
        echo "接口 $WG_IFACE: 未启动"
    fi
}

do_uninstall() {
    check_root
    echo -e "${YELLOW}卸载 WireGuard Server? (y/N)${NC}"
    read -r yn
    [[ "$yn" != "y" && "$yn" != "Y" ]] && { info "已取消"; exit 0; }
    systemctl stop wg-quick@wg0 2>/dev/null || true
    systemctl disable wg-quick@wg0 2>/dev/null || true
    rm -f "$WG_CONF" "$WG_DIR"/server.key "$WG_DIR"/server.pub "$WG_DIR"/client-*.conf
    apt-get remove -y -qq wireguard-tools 2>/dev/null || true
    info "已卸载"
}

case "$ACTION" in
    help)      show_help ;;
    install)   do_install ;;
    add-peer)  do_add_peer "${2:-}" ;;
    peers)     do_peers ;;
    status)    do_status ;;
    uninstall) do_uninstall ;;
esac