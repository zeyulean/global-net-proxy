#!/bin/bash
#===============================================================
# global-net-proxy — sing-box Client 安装/管理脚本 (跨平台)
#
# ⚠️  安全原则: 本脚本只使用 mixed 代理模式, 绝不使用 tun 模式。
#     tun 模式 (strict_route + auto_route) 会接管系统路由表,
#     在无带外访问的机器上会导致完全断网 (见 docs/incident-2026-08-10.md)。
#     mixed 模式只监听 0.0.0.0:1080 (socks5+http), 不碰路由表, 零风险。
#
# 架构: sing-box 二进制
#   - inbound:   mixed (listen 0.0.0.0:1080, socks5+http 代理端口)
#   - dns:       分流 (国内→223.5.5.5 直连, 国外→1.1.1.1 走 wg)
#   - endpoint:  wireguard (userspace, system:false, 连接远端 wg server)
#   - route:     ip_is_private → direct, final → wg-ep
#
# 支持平台: Ubuntu / Debian (Linux), macOS, Windows (Git Bash / WSL)
#
# 用法:
#   bash client.sh --install          # 安装 sing-box 并生成配置(交互)
#   bash client.sh --install-auto     # 全自动: 用环境变量/参数, 不交互
#   bash client.sh --test             # 先跑 10 秒测试, 验证配置无错
#   bash client.sh --start            # 启动 sing-box (常驻)
#   bash client.sh --stop             # 停止
#   bash client.sh --status           # 查看状态
#   bash client.sh --uninstall        # 卸载
#   bash client.sh --help, -h         # 帮助
#
# 安装参数(交互式或 --install-auto 时):
#   SERVER=远程wg服务器地址  WG_PUBKEY=对端公钥  CLIENT_PRIVKEY=本机私钥
#   CLIENT_IP=本机内网IP(如 10.0.0.5/32)  WG_PORT=端口(默认51820)
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

SB_VERSION="${SB_VERSION:-1.13.16}"
SB_DIR="$HOME/.local/share/sing-box"
SB_BIN="$SB_DIR/sing-box"
SB_CONF="$SB_DIR/config.json"
SB_LOG="$SB_DIR/sing-box.log"
SB_PID="$SB_DIR/sing-box.pid"
RULE_DIR="$SB_DIR/rules"
RULESET_GEOIP_CN="https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geoip/geoip-cn.srs"
RULESET_GEOSITE_CN="https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-cn.srs"
FOREIGN_GROUPS="google github openai anthropic docker"
FOREIGN_DOMAINS="pypi.org files.pythonhosted.org registry.npmjs.org registry.yarnpkg.com crates.io static.crates.io proxy.golang.org go.dev repo.maven.apache.org search.maven.org maven.apache.org"

# systemd service 名称 (Linux)
SERVICE_NAME="sing-box-gnp"

#---------------------------------------------------------------
ACTION="${1:-help}"
case "$ACTION" in
    --install|-i)      ACTION="install" ;;
    --install-auto)    ACTION="install-auto" ;;
    --test)            ACTION="test" ;;
    --start)           ACTION="start" ;;
    --stop)            ACTION="stop" ;;
    --status)          ACTION="status" ;;
    --uninstall)       ACTION="uninstall" ;;
    --help|-h)         ACTION="help" ;;
    *)                 ACTION="help" ;;
esac

show_help() {
    echo "global-net-proxy — sing-box Client (mixed 代理模式, 安全)"
    echo ""
    echo "⚠️  本脚本只使用 mixed 代理模式, 绝不使用 tun 模式。"
    echo "    代理端口: socks5+http 0.0.0.0:1080 (不碰路由表, 零断网风险)"
    echo ""
    echo "用法: bash client.sh [选项]"
    echo "  --install        交互式安装 (检测平台/下载sing-box/生成配置)"
    echo "  --install-auto   全自动安装 (需环境变量, 见下)"
    echo "  --test           先跑 10 秒测试, 验证配置无错再决定是否 install"
    echo "  --start          启动 sing-box (后台常驻)"
    echo "  --stop           停止"
    echo "  --status         查看状态"
    echo "  --uninstall      卸载"
    echo "  --help, -h       帮助"
    echo ""
    echo "安装所需参数(--install 交互输入 或 环境变量):"
    echo "  SERVER          远程 wg server 地址 (IP 或域名)"
    echo "  WG_PUBKEY       对端(server)公钥"
    echo "  CLIENT_PRIVKEY  本机私钥"
    echo "  CLIENT_IP       本机内网IP (如 10.0.0.5/32)"
    echo "  WG_PORT         端口 (默认51820)"
    echo "  SB_VERSION      sing-box 版本 (默认1.13.16)"
    echo ""
    echo "使用代理 (sing-box 启动后):"
    echo "  export http_proxy=http://127.0.0.1:1080"
    echo "  export https_proxy=http://127.0.0.1:1080"
    echo "  export all_proxy=socks5://127.0.0.1:1080"
    exit 0
}

#---------------------------------------------------------------
# 平台检测
detect_platform() {
    case "$(uname -s)" in
        Linux)
            if grep -qi microsoft /proc/version 2>/dev/null; then
                PLATFORM="wsl"
            else
                PLATFORM="linux"
            fi
            ;;
        Darwin) PLATFORM="macos" ;;
        MINGW*|MSYS*|CYGWIN*) PLATFORM="windows" ;;
        *) error "不支持的系统: $(uname -s)" ;;
    esac
    info "检测到平台: $PLATFORM"
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  ARCH="amd64" ;;
        aarch64|arm64) ARCH="arm64" ;;
        armv7l)        ARCH="armv7" ;;
        *) error "不支持的架构: $(uname -m)" ;;
    esac
    info "架构: $ARCH"
}

#---------------------------------------------------------------
# 下载并安装 sing-box
install_singbox() {
    if [[ -x "$SB_BIN" ]]; then
        info "sing-box 已安装: $SB_BIN"
        "$SB_BIN" version 2>/dev/null | head -1 || true
        return
    fi
    mkdir -p "$SB_DIR" "$RULE_DIR"

    local url=""
    case "$PLATFORM" in
        linux)  url="https://github.com/SagerNet/sing-box/releases/download/v${SB_VERSION}/sing-box-${SB_VERSION}-linux-${ARCH}.tar.gz" ;;
        macos)  url="https://github.com/SagerNet/sing-box/releases/download/v${SB_VERSION}/sing-box-${SB_VERSION}-darwin-${ARCH}.tar.gz" ;;
        windows) url="https://github.com/SagerNet/sing-box/releases/download/v${SB_VERSION}/sing-box-${SB_VERSION}-windows-${ARCH}.zip" ;;
        wsl)    url="https://github.com/SagerNet/sing-box/releases/download/v${SB_VERSION}/sing-box-${SB_VERSION}-linux-${ARCH}.tar.gz" ;;
    esac

    info "下载 sing-box v${SB_VERSION}: $url"
    local tmp="$SB_DIR/.download"
    mkdir -p "$tmp"
    case "$PLATFORM" in
        windows)
            curl -fL --retry 3 -o "$tmp/sb.zip" "$url"
            (cd "$tmp" && unzip -o sb.zip >/dev/null)
            find "$tmp" -name 'sing-box.exe' -exec mv {} "$SB_BIN" \;
            ;;
        *)
            curl -fL --retry 3 -o "$tmp/sb.tar.gz" "$url"
            tar -xzf "$tmp/sb.tar.gz" -C "$tmp"
            find "$tmp" -name 'sing-box' -type f -exec mv {} "$SB_BIN" \;
            ;;
    esac
    chmod +x "$SB_BIN"
    rm -rf "$tmp"
    info "sing-box 安装完成: $SB_BIN"
    "$SB_BIN" version 2>/dev/null | head -1 || true
}

#---------------------------------------------------------------
# 下载规则集 (预下载到本地, 供本地 rule_set 引用)
download_rules() {
    info "下载规则集到 $RULE_DIR ..."
    for g in $FOREIGN_GROUPS; do
        local url="https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-${g}.srs"
        if curl -fsSL --max-time 20 -o "$RULE_DIR/geosite-${g}.srs" "$url" 2>/dev/null; then
            info "  ✓ geosite-${g}"
        else
            warn "  ✗ geosite-${g} 下载失败 (跳过)"
        fi
    done
    curl -fsSL --max-time 30 -o "$RULE_DIR/geoip-cn.srs" "$RULESET_GEOIP_CN" && info "  ✓ geoip-cn" || warn "  ✗ geoip-cn"
    curl -fsSL --max-time 30 -o "$RULE_DIR/geosite-cn.srs" "$RULESET_GEOSITE_CN" && info "  ✓ geosite-cn" || warn "  ✗ geosite-cn"
}

#---------------------------------------------------------------
# 生成 sing-box 配置 (mixed 代理模式 — 安全, 不碰路由表)
gen_config() {
    local server="$SERVER" pubkey="$WG_PUBKEY" privkey="$CLIENT_PRIVKEY" ip="$CLIENT_IP"
    [[ -z "$server" ]]   && read -rp "远程 wg server 地址: " server
    [[ -z "$pubkey" ]]   && read -rp "对端(server)公钥: " pubkey
    [[ -z "$privkey" ]]  && read -rp "本机私钥: " privkey
    [[ -z "$ip" ]]       && read -rp "本机内网IP(如 10.0.0.5/32): " ip
    local port="${WG_PORT:-51820}"

    # 构造 rule_set 定义 (remote, download_detour=direct 避免鸡蛋问题)
    local rulesets=""
    for g in $FOREIGN_GROUPS; do
        rulesets+="      { \"type\": \"remote\", \"tag\": \"geosite-${g}\", \"format\": \"binary\", \"url\": \"https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-${g}.srs\", \"download_detour\": \"direct\" },
"
    done
    rulesets+="      { \"type\": \"remote\", \"tag\": \"geoip-cn\", \"format\": \"binary\", \"url\": \"$RULESET_GEOIP_CN\", \"download_detour\": \"direct\" },
      { \"type\": \"remote\", \"tag\": \"geosite-cn\", \"format\": \"binary\", \"url\": \"$RULESET_GEOSITE_CN\", \"download_detour\": \"direct\" }"

    # 构造 route 规则 — 国外 geosite → wg-ep
    local wg_rules=""
    for g in $FOREIGN_GROUPS; do
        wg_rules+="        { \"rule_set\": \"geosite-${g}\", \"outbound\": \"wg-ep\" },
"
    done
    # domain 规则 (包管理站等无 geosite 分类的目标)
    local domain_json=""
    for d in $FOREIGN_DOMAINS; do domain_json+="\"${d}\", "; done
    domain_json="${domain_json%, }"
    wg_rules+="        { \"domain\": [ ${domain_json} ], \"outbound\": \"wg-ep\" },
"
    # 国内 geosite/geoip → direct
    wg_rules+="        { \"rule_set\": [ \"geoip-cn\", \"geosite-cn\" ], \"outbound\": \"direct\" }"

    cat > "$SB_CONF" <<EOF
{
  "log": { "level": "info", "timestamp": true },
  "dns": {
    "servers": [
      { "tag": "dns-proxy", "type": "https", "server": "1.1.1.1", "detour": "wg-ep" },
      { "tag": "dns-direct", "type": "udp", "server": "223.5.5.5" }
    ],
    "rules": [
      { "rule_set": [ "geosite-cn\", \"geoip-cn\" ], \"server\": \"dns-direct\" }
    ],
    "final": "dns-proxy",
    "strategy": "prefer_ipv4"
  },
  "endpoints": [
    {
      "type": "wireguard",
      "tag": "wg-ep",
      "system": false,
      "mtu": 1280,
      "address": [ "${ip}" ],
      "private_key": "${privkey}",
      "peers": [
        {
          "address": "${server}",
          "port": ${port},
          "public_key": "${pubkey}",
          "allowed_ips": [ "0.0.0.0/0" ],
          "persistent_keepalive_interval": 25
        }
      ]
    }
  ],
  "inbounds": [
    {
      "type": "mixed",
      "tag": "mixed-in",
      "listen": "0.0.0.0",
      "listen_port": 1080
    }
  ],
  "outbounds": [
    { "type": "direct", "tag": "direct" }
  ],
  "route": {
    "rule_set": [
${rulesets}
    ],
    "rules": [
      { "ip_is_private": true, "outbound": "direct" },
${wg_rules}
    ],
    "final": "wg-ep",
    "default_domain_resolver": "dns-direct"
  }
}
EOF
    info "配置已生成: $SB_CONF"
    info "代理端口: socks5+http 0.0.0.0:1080 (mixed 模式, 不碰路由表)"
}

#---------------------------------------------------------------
# 生成 systemd service (Linux/wsl) — 非 root, on-failure
gen_systemd_service() {
    [[ "$PLATFORM" != "linux" && "$PLATFORM" != "wsl" ]] && return 0

    local svc_path="$HOME/.config/systemd/user/${SERVICE_NAME}.service"
    mkdir -p "$(dirname "$svc_path")"

    cat > "$svc_path" <<EOF
[Unit]
Description=global-net-proxy sing-box (mixed proxy mode)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=${SB_BIN} run -c ${SB_CONF}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF
    info "systemd --user service 已生成: $svc_path"
    info "  User=$(whoami) (非 root), Restart=on-failure (非 always)"
    echo ""
    echo "  启用:  systemctl --user daemon-reload && systemctl --user enable --now ${SERVICE_NAME}"
    echo "  状态:  systemctl --user status ${SERVICE_NAME}"
    echo "  日志:  journalctl --user -u ${SERVICE_NAME} -f"
}

#---------------------------------------------------------------
# 测试: 先跑 10 秒验证配置无错 (不常驻, 不改系统)
do_test() {
    [[ -f "$SB_CONF" ]] || error "未生成配置, 先 --install"
    [[ -x "$SB_BIN" ]] || error "sing-box 未安装: $SB_BIN"

    info "测试运行 sing-box (10 秒后自动退出)..."
    info "  配置: $SB_CONF"
    info "  mixed 模式不碰路由表, 断网风险为零"

    # 先验证配置格式
    if ! "$SB_BIN" check -c "$SB_CONF" 2>&1; then
        error "配置验证失败! 请检查 $SB_CONF"
    fi
    info "✓ 配置格式验证通过"

    # 运行 10 秒
    timeout 10 "$SB_BIN" run -c "$SB_CONF" 2>&1 | head -30 || true
    echo ""
    info "✓ 10 秒测试完成 (mixed 模式不接管路由, 安全)"
    info "  如需安装: bash $0 --install"
}

#---------------------------------------------------------------
# 启动 (后台, 按平台)
do_start() {
    [[ -f "$SB_CONF" ]] || error "未安装/未生成配置, 先 --install"

    # mixed 模式不需要 root (不建 tun, 不碰路由表)
    "$SB_BIN" run -c "$SB_CONF" >"$SB_LOG" 2>&1 &
    local pid=$!
    echo "$pid" > "$SB_PID"
    sleep 1
    if kill -0 "$pid" 2>/dev/null; then
        info "sing-box 启动成功, PID=$pid (日志: $SB_LOG)"
        info "代理端口: socks5+http 127.0.0.1:1080"
    else
        error "sing-box 启动失败, 查看日志: $SB_LOG"
    fi
}

do_stop() {
    if [[ -f "$SB_PID" ]]; then
        kill "$(cat "$SB_PID")" 2>/dev/null && info "已停止" || warn "进程不存在"
        rm -f "$SB_PID"
    else
        pkill -f "sing-box run" 2>/dev/null && info "已停止" || warn "未在运行"
    fi
}

do_status() {
    if pgrep -f "sing-box run" >/dev/null 2>&1; then
        echo -e "${GREEN}sing-box: 运行中 ✓${NC}"
        [[ -f "$SB_PID" ]] && echo "  PID: $(cat "$SB_PID")"
        echo "  代理: socks5+http 0.0.0.0:1080 (mixed 模式)"
        tail -5 "$SB_LOG" 2>/dev/null || true
    else
        echo "sing-box: 未运行"
    fi
}

do_uninstall() {
    do_stop
    # Linux: 同时清理 systemd service
    if [[ "$PLATFORM" == "linux" || "$PLATFORM" == "wsl" ]]; then
        local svc_path="$HOME/.config/systemd/user/${SERVICE_NAME}.service"
        if [[ -f "$svc_path" ]]; then
            systemctl --user disable "${SERVICE_NAME}" 2>/dev/null || true
            rm -f "$svc_path"
            systemctl --user daemon-reload 2>/dev/null || true
            info "已清理 systemd service"
        fi
    fi
    read -rp "删除全部(sing-box + config + rules)? [y/N] " yn
    if [[ "$yn" == "y" || "$yn" == "Y" ]]; then
        rm -rf "$SB_DIR"
        info "已彻底卸载"
    else
        info "已停止, 保留 $SB_DIR"
    fi
}

#===============================================================
case "$ACTION" in
    help) show_help ;;
    test)
        detect_platform
        detect_arch
        do_test ;;
    install)
        detect_platform
        detect_arch
        install_singbox
        download_rules
        gen_config
        gen_systemd_service
        info "安装完成!"
        info "  测试:   bash $0 --test"
        info "  启动:   bash $0 --start"
        ;;
    install-auto)
        detect_platform
        detect_arch
        [[ -z "$SERVER" || -z "$WG_PUBKEY" || -z "$CLIENT_PRIVKEY" || -z "$CLIENT_IP" ]] && \
            error "install-auto 需要环境变量: SERVER WG_PUBKEY CLIENT_PRIVKEY CLIENT_IP"
        install_singbox
        download_rules
        gen_config
        gen_systemd_service
        info "安装完成!"
        info "  测试:   bash $0 --test"
        info "  启动:   bash $0 --start"
        ;;
    start)
        detect_platform
        do_start ;;
    stop)    do_stop ;;
    status)  do_status ;;
    uninstall)
        detect_platform
        do_uninstall ;;
esac
