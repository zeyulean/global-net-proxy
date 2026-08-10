#!/bin/bash
#===============================================================
# global-net-proxy — sing-box Client 安装/管理脚本 (跨平台)
#
# 架构: sing-box 二进制
#   - inbound:  tun (接管系统流量)
#   - dns:      分流 (国内域名→国内DNS直连, 国外域名→wg隧道远端解析)
#   - endpoint:  wireguard (连接远端 wg server)
#   - route:     geosite 命中(google/github/代码源/AI) → wg; 其余 → direct
#
# 支持平台: Ubuntu / Debian (Linux), macOS, Windows (Git Bash / WSL)
# 自动检测, 跨平台单一入口。
#
# 用法:
#   bash client.sh --install          # 安装 sing-box 并生成配置(交互)
#   bash client.sh --install-auto     # 全自动: 用环境变量/参数, 不交互
#   bash client.sh --start            # 启动 sing-box (常驻)
#   bash client.sh --stop             # 停止
#   bash client.sh --status           # 查看状态
#   bash client.sh --uninstall        # 卸载
#   bash client.sh --help, -h         # 帮助
#
# 安装参数(交互式或 --install-auto 时):
#   SERVER=远程wg服务器地址  WG_PUBKEY=对端公钥  CLIENT_PRIVKEY=本机私钥
#   CLIENT_IP=本机内网IP(如 10.99.0.2/32)  WG_PORT=端口(默认51820)
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

SB_VERSION="${SB_VERSION:-1.13.16}"
# sing-box 安装目录
SB_DIR="$HOME/.local/share/sing-box"
SB_BIN="$SB_DIR/sing-box"
SB_CONF="$SB_DIR/config.json"
SB_LOG="$SB_DIR/sing-box.log"
SB_PID="$SB_DIR/sing-box.pid"
# 规则集目录
RULE_DIR="$SB_DIR/rules"
# 上游规则集 (lyc8503/sing-box-rules, GitHub Actions 每日自动更新)
RULESET_GEOIP_CN="https://raw.githubusercontent.com/lyc8503/sing-box-rules/sing-geoip/geoip-cn.srs"
RULESET_GEOSITE_CN="https://raw.githubusercontent.com/lyc8503/sing-box-rules/sing-geosite/geosite-cn.srs"
# 常见国外目标 geosite 分类 (都走 wg)
FOREIGN_GROUPS="google github openai anthropic pypi npm crates go-repository maven docker"

#---------------------------------------------------------------
ACTION="${1:-help}"
case "$ACTION" in
    --install|-i)      ACTION="install" ;;
    --install-auto)    ACTION="install-auto" ;;
    --start)           ACTION="start" ;;
    --stop)            ACTION="stop" ;;
    --status)          ACTION="status" ;;
    --uninstall)       ACTION="uninstall" ;;
    --help|-h)         ACTION="help" ;;
    *)                 ACTION="help" ;;
esac

show_help() {
    echo "global-net-proxy — sing-box Client (Linux/macOS/Windows)"
    echo ""
    echo "用法: bash client.sh [选项]"
    echo "  --install        交互式安装 (检测平台/下载sing-box/生成配置)"
    echo "  --install-auto   全自动安装 (需环境变量, 见下)"
    echo "  --start          启动 sing-box (后台常驻)"
    echo "  --stop           停止"
    echo "  --status         查看状态"
    echo "  --uninstall      卸载"
    echo "  --help, -h       帮助"
    echo ""
    echo "安装所需参数(--install 交互输入 或 环境变量):"
    echo "  SERVER        远程 wg server 地址 (IP 或域名)"
    echo "  WG_PUBKEY     对端(server)公钥"
    echo "  CLIENT_PRIVKEY 本机私钥"
    echo "  CLIENT_IP     本机内网IP (如 10.99.0.2/32)"
    echo "  WG_PORT       端口 (默认51820)"
    echo "  SB_VERSION    sing-box 版本 (默认1.13.16)"
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

# 架构
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
# 下载规则集 (国外目标 geosite 分组 + 国内 geoip/geosite)
download_rules() {
    info "下载规则集到 $RULE_DIR ..."
    # 国外目标分组 → 全部走 wg
    for g in $FOREIGN_GROUPS; do
        local url="https://raw.githubusercontent.com/lyc8503/sing-box-rules/sing-geosite/geosite-${g}.srs"
        if curl -fsSL --max-time 20 -o "$RULE_DIR/geosite-${g}.srs" "$url" 2>/dev/null; then
            info "  ✓ geosite-${g}"
        else
            warn "  ✗ geosite-${g} 下载失败 (跳过)"
        fi
    done
    # 国内 geoip (用于国内IP直连)
    curl -fsSL --max-time 30 -o "$RULE_DIR/geoip-cn.srs" "$RULESET_GEOIP_CN" && info "  ✓ geoip-cn" || warn "  ✗ geoip-cn"
    # 国内 geosite (国内域名直连)
    curl -fsSL --max-time 30 -o "$RULE_DIR/geosite-cn.srs" "$RULESET_GEOSITE_CN" && info "  ✓ geosite-cn" || warn "  ✗ geosite-cn"
}

#---------------------------------------------------------------
# 生成 sing-box 配置
gen_config() {
    local server="$SERVER" pubkey="$WG_PUBKEY" privkey="$CLIENT_PRIVKEY" ip="$CLIENT_IP"
    [[ -z "$server" ]]   && read -rp "远程 wg server 地址: " server
    [[ -z "$pubkey" ]]   && read -rp "对端(server)公钥: " pubkey
    [[ -z "$privkey" ]]  && read -rp "本机私钥: " privkey
    [[ -z "$ip" ]]       && read -rp "本机内网IP(如 10.99.0.2/32): " ip
    local port="${WG_PORT:-51820}"

    # 生成规则集定义 (remote 类型 → sing-box 启动下载 + 默认24h自动更新)
    local rulesets=""
    for g in $FOREIGN_GROUPS; do
        rulesets+="  { \"type\": \"remote\", \"tag\": \"geosite-${g}\", \"format\": \"binary\", \"url\": \"https://raw.githubusercontent.com/lyc8503/sing-box-rules/sing-geosite/geosite-${g}.srs\", \"download_detour\": \"wg-out\" },
"
    done
    rulesets+="  { \"type\": \"remote\", \"tag\": \"geoip-cn\", \"format\": \"binary\", \"url\": \"$RULESET_GEOIP_CN\", \"download_detour\": \"wg-out\" },
  { \"type\": \"remote\", \"tag\": \"geosite-cn\", \"format\": \"binary\", \"url\": \"$RULESET_GEOSITE_CN\", \"download_detour\": \"wg-out\" }"

    # 生成 route 规则
    local wg_rules=""
    for g in $FOREIGN_GROUPS; do
        wg_rules+="      { \"rule_set\": \"geosite-${g}\", \"outbound\": \"wg-out\" },
"
    done

    cat > "$SB_CONF" <<EOF
{
  "log": { "level": "info", "timestamp": true },
  "dns": {
    "servers": [
      { "tag": "dns-remote", "address": "https://1.1.1.1/dns-query", "detour": "wg-out" },
      { "tag": "dns-direct", "address": "223.5.5.5", "detour": "direct" }
    ],
    "rules": [
      { "rule_set": [ "geosite-cn", "geoip-cn" ], "server": "dns-direct" },
      { "action": "route", "server": "dns-remote" }
    ],
    "final": "dns-direct",
    "independent_cache": true
  },
  "inbounds": [
    {
      "type": "tun",
      "tag": "tun-in",
      "address": [ "172.19.0.1/30" ],
      "auto_route": true,
      "strict_route": true,
      "stack": "mixed"
    }
  ],
  "endpoints": [
    {
      "type": "wireguard",
      "tag": "wg-out",
      "system": true,
      "name": "gnp0",
      "address": [ "${ip}" ],
      "private_key": "${privkey}",
      "mtu": 1408,
      "peers": [
        {
          "address": "${server}",
          "port": ${port},
          "public_key": "${pubkey}",
          "allowed_ips": [ "0.0.0.0/0", "::/0" ],
          "persistent_keepalive_interval": 25
        }
      ]
    }
  ],
  "route": {
    "final": "direct",
    "rules": [
${wg_rules}
      { "rule_set": "geoip-cn", "outbound": "direct" },
      { "rule_set": "geosite-cn", "outbound": "direct" }
    ]
  },
  "route.rule_set": [
${rulesets}
  ]
}
EOF
    info "配置已生成: $SB_CONF"
}

#---------------------------------------------------------------
# 平台特有的启动/停止
platform_start() {
    case "$PLATFORM" in
        linux|wsl)
            # 需要 root 建 tun
            if [[ $EUID -ne 0 ]]; then
                warn "Linux 需要 root 启动 (tun); 尝试 sudo..."
                sudo -n "$SB_BIN" run -c "$SB_CONF" >"$SB_LOG" 2>&1 &
                SPID=$!
            else
                "$SB_BIN" run -c "$SB_CONF" >"$SB_LOG" 2>&1 &
                SPID=$!
            fi
            ;;
        macos)
            # macOS tun 需要 root
            if [[ $EUID -ne 0 ]]; then
                warn "macOS 需要 root 启动 (tun); 尝试 sudo..."
                sudo -n "$SB_BIN" run -c "$SB_CONF" >"$SB_LOG" 2>&1 &
                SPID=$!
            else
                "$SB_BIN" run -c "$SB_CONF" >"$SB_LOG" 2>&1 &
                SPID=$!
            fi
            ;;
        windows)
            # Git Bash 下无法方便后台, 提示用任务计划
            warn "Windows 请用 PowerShell/任务计划运行 sing-box.exe run -c config.json"
            warn "或按住 Ctrl 后运行, 保持前台"
            return 1
            ;;
    esac
    echo "$SPID" > "$SB_PID"
    info "sing-box 启动, PID=$SPID (日志: $SB_LOG)"
}

do_start() {
    [[ -f "$SB_CONF" ]] || error "未安装/未生成配置, 先 --install"
    platform_start
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
        tail -5 "$SB_LOG" 2>/dev/null || true
    else
        echo "sing-box: 未运行"
    fi
}

do_uninstall() {
    do_stop
    # 保留 config 供备份, 询问
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
    install)
        detect_platform
        detect_arch
        install_singbox
        download_rules
        gen_config
        info "安装完成! 运行: bash $0 --start"
        ;;
    install-auto)
        detect_platform
        detect_arch
        [[ -z "$SERVER" || -z "$WG_PUBKEY" || -z "$CLIENT_PRIVKEY" || -z "$CLIENT_IP" ]] && \
            error "install-auto 需要环境变量: SERVER WG_PUBKEY CLIENT_PRIVKEY CLIENT_IP"
        install_singbox
        download_rules
        gen_config
        info "安装完成! 运行: bash $0 --start"
        ;;
    start)   do_start ;;
    stop)    do_stop ;;
    status)  do_status ;;
    uninstall) do_uninstall ;;
esac