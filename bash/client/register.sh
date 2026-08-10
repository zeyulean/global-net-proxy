#!/bin/bash
#===============================================================
# global-net-proxy — 新机器一键注册脚本
#
# 从 gitee 私有仓库拉取预生成的 peer 配置池, 挑一个未使用的,
# 自动生成 sing-box config.json 并安装。
#
# 前提:
#   - 环境变量 GITEE_TOKEN 已设置 (有 gitee.com/lw_boy/global-net-proxy 读权限)
#   - 管理员已在 lwtop 上运行过 server.sh --pre-gen <N>
#
# 用法:
#   export GITEE_TOKEN=xxxx
#   bash register.sh [client_id]          # 注册 (client_id 可选, 默认用 hostname)
#   bash register.sh ningsure             # 指定 client_id
#   bash register.sh --list               # 列出可用的 peer
#   bash register.sh --dry-run [id]       # 只看会选中哪个, 不实际修改
#   bash register.sh --help, -h           # 帮助
#
# 注册后会提示用户去 lwtop 执行 server.sh --activate <client_id>
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

#===============================================================
# 常量
#===============================================================

# gitee 私有仓库
GITEE_REPO="lw_boy/global-net-proxy"
GITEE_BRANCH="${GITEE_BRANCH:-main}"

# lwtop server 配置 (公钥可公开)
SERVER_HOST="8.209.203.17"
SERVER_PORT="51820"
SERVER_PUBKEY="M/t3YYwIW7Xou+vASGtNoAHHNrh82ROzYDU4LIsLz18="

# sing-box 版本
SB_VERSION="${SB_VERSION:-1.13.16}"
SB_DIR="$HOME/.local/share/sing-box"
SB_BIN="$SB_DIR/sing-box"
SB_CONF="$SB_DIR/config.json"
SB_LOG="$SB_DIR/sing-box.log"
SB_PID="$SB_DIR/sing-box.pid"
RULE_DIR="$SB_DIR/rules"
SERVICE_NAME="sing-box-gnp"

# 规则集 URL
RULESET_GEOIP_CN="https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geoip/geoip-cn.srs"
RULESET_GEOSITE_CN="https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-cn.srs"
FOREIGN_GROUPS="google github openai anthropic docker"
FOREIGN_DOMAINS="pypi.org files.pythonhosted.org registry.npmjs.org registry.yarnpkg.com crates.io static.crates.io proxy.golang.org go.dev repo.maven.apache.org search.maven.org maven.apache.org"

#===============================================================
# 参数解析
#===============================================================
ACTION="${1:-register}"
CLIENT_ID=""
DRY_RUN=false

case "$ACTION" in
    --help|-h)  ACTION="help" ;;
    --list)     ACTION="list" ;;
    --dry-run)  ACTION="register"; DRY_RUN=true; CLIENT_ID="${2:-}" ;;
    *)          ACTION="register"; CLIENT_ID="$ACTION" ;;
esac

show_help() {
    echo "global-net-proxy — 新机器一键注册"
    echo ""
    echo "用法:"
    echo "  export GITEE_TOKEN=xxxx"
    echo "  bash register.sh [client_id]    # 注册 (client_id 可选)"
    echo "  bash register.sh ningsure       # 指定 client_id"
    echo "  bash register.sh --list         # 列出可用 peer"
    echo "  bash register.sh --dry-run [id] # 只看, 不修改"
    echo "  bash register.sh --help         # 帮助"
    echo ""
    echo "注册完成后, 需在 lwtop 上执行:"
    echo "  sudo bash server.sh --activate <client_id>"
    echo ""
    echo "环境变量:"
    echo "  GITEE_TOKEN   gitee 私有仓库访问 token (必须)"
    echo "  SB_VERSION    sing-box 版本 (默认 1.13.16)"
    exit 0
}

#===============================================================
# 平台检测
#===============================================================
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
    info "平台: $PLATFORM"
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

#===============================================================
# Gitee 仓库操作
#===============================================================

# 检查 GITEE_TOKEN
check_token() {
    [[ -z "${GITEE_TOKEN:-}" ]] && error "GITEE_TOKEN 未设置!
请设置环境变量: export GITEE_TOKEN=xxxx"
}

# 构造 gitee clone URL (带 token)
gitee_clone_url() {
    echo "https://oauth2:${GITEE_TOKEN}@gitee.com/${GITEE_REPO}.git"
}

# 克隆仓库到临时目录, 返回路径
clone_repo() {
    local tmpdir
    tmpdir=$(mktemp -d /tmp/gnp-repo-XXXXXX)
    info "从 gitee 克隆仓库..."
    git clone --depth 1 --branch "$GITEE_BRANCH" \
        "$(gitee_clone_url)" "$tmpdir" 2>&1 | grep -v 'oauth2' || true
    echo "$tmpdir"
}

# 列出 peers/ 目录中的可用 peer
do_list() {
    check_token
    local repo_dir
    repo_dir=$(clone_repo)
    trap "rm -rf $repo_dir" EXIT

    local peers_dir="$repo_dir/peers"
    if [[ ! -d "$peers_dir" ]]; then
        error "peers/ 目录不存在。请先在 lwtop 上运行: sudo bash server.sh --pre-gen <N>"
    fi

    echo -e "${CYAN}===== Peer 池状态 =====${NC}"
    echo ""

    local available=0 used=0 activated=0
    for f in "$peers_dir"/*.json; do
        [[ -f "$f" ]] || continue
        local cid wip status
        cid=$(grep -oP '"client_id"\s*:\s*"\K[^"]+' "$f")
        wip=$(grep -oP '"wg_ip"\s*:\s*"\K[^"]+' "$f")
        status=$(grep -oP '"status"\s*:\s*"\K[^"]+' "$f")
        case "$status" in
            available)  echo -e "  ${GREEN}✓${NC} $cid  $wip  [$status]"; available=$((available+1)) ;;
            used)       echo -e "  ${YELLOW}●${NC} $cid  $wip  [$status]"; used=$((used+1)) ;;
            activated)  echo -e "  ${CYAN}★${NC} $cid  $wip  [$status]"; activated=$((activated+1)) ;;
            *)          echo -e "  ? $cid  $wip  [$status]" ;;
        esac
    done

    echo ""
    info "总计: $available available, $used used, $activated activated"
}

#===============================================================
# 选择 peer
#===============================================================

# 在 peers/ 目录中选择一个 peer
# 优先级: client_id 匹配 > 任意 available
# 参数: $1=peers_dir  $2=client_id(可选)
# 输出: 选中 peer 的文件路径
select_peer() {
    local peers_dir="$1" client_id="${2:-}"

    # 如果指定了 client_id, 先尝试精确匹配
    if [[ -n "$client_id" ]]; then
        local match="$peers_dir/${client_id}.json"
        if [[ -f "$match" ]]; then
            local status
            status=$(grep -oP '"status"\s*:\s*"\K[^"]+' "$match")
            if [[ "$status" == "available" ]]; then
                echo "$match"
                return 0
            else
                error "peer $client_id 状态为 '$status' (非 available), 可能已被使用"
            fi
        fi
        # 没有精确匹配的文件, 继续找 available 的 (后面 rename)
    fi

    # 找第一个 available 的 peer
    for f in "$peers_dir"/*.json; do
        [[ -f "$f" ]] || continue
        local status
        status=$(grep -oP '"status"\s*:\s*"\K[^"]+' "$f")
        if [[ "$status" == "available" ]]; then
            echo "$f"
            return 0
        fi
    done

    return 1
}

# 标记 peer 为 used (修改 JSON + git push)
# 参数: $1=peer_file  $2=client_id
mark_peer_used() {
    local peer_file="$1" client_id="$2" repo_dir="$3"
    local timestamp
    timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

    sed -i "s/\"status\": \"available\"/\"status\": \"used\"/" "$peer_file"
    # 如果 client_id 和文件名不同, 更新 JSON 中的 client_id 字段
    local orig_id
    orig_id=$(grep -oP '"client_id"\s*:\s*"\K[^"]+' "$peer_file")
    if [[ "$orig_id" != "$client_id" ]]; then
        sed -i "s/\"client_id\": \"${orig_id}\"/\"client_id\": \"${client_id}\"/" "$peer_file"
    fi

    info "标记 peer $client_id 为 used..."
    (
        cd "$repo_dir"
        git config user.email "register@global-net-proxy" 2>/dev/null || true
        git config user.name "register.sh" 2>/dev/null || true
        git add -A
        git commit -m "register: $client_id marked as used" >/dev/null 2>&1 || true
        git push origin "$GITEE_BRANCH" 2>&1 | grep -v 'oauth2' || true
    )
    info "✓ 已标记并推送到 gitee"
}

#===============================================================
# sing-box 安装 (复用 client.sh 逻辑)
#===============================================================

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

    info "下载 sing-box v${SB_VERSION}..."
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
    info "✓ sing-box 安装完成"
    "$SB_BIN" version 2>/dev/null | head -1 || true
}

download_rules() {
    info "下载规则集..."
    for g in $FOREIGN_GROUPS; do
        local url="https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-${g}.srs"
        if curl -fsSL --max-time 20 -o "$RULE_DIR/geosite-${g}.srs" "$url" 2>/dev/null; then
            info "  ✓ geosite-${g}"
        else
            warn "  ✗ geosite-${g} (跳过)"
        fi
    done
    curl -fsSL --max-time 30 -o "$RULE_DIR/geoip-cn.srs" "$RULESET_GEOIP_CN" && info "  ✓ geoip-cn" || warn "  ✗ geoip-cn"
    curl -fsSL --max-time 30 -o "$RULE_DIR/geosite-cn.srs" "$RULESET_GEOSITE_CN" && info "  ✓ geosite-cn" || warn "  ✗ geosite-cn"
}

#===============================================================
# 生成 sing-box config.json (mixed 代理模式)
#===============================================================
# 参数: private_key  wg_ip  (server_host/port/pubkey 用常量)
gen_config() {
    local privkey="$1" wg_ip="$2"

    # 构造 rule_set 定义
    local rulesets=""
    for g in $FOREIGN_GROUPS; do
        rulesets+="      { \"type\": \"remote\", \"tag\": \"geosite-${g}\", \"format\": \"binary\", \"url\": \"https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-${g}.srs\", \"download_detour\": \"direct\" },
"
    done
    rulesets+="      { \"type\": \"remote\", \"tag\": \"geoip-cn\", \"format\": \"binary\", \"url\": \"$RULESET_GEOIP_CN\", \"download_detour\": \"direct\" },
      { \"type\": \"remote\", \"tag\": \"geosite-cn\", \"format\": \"binary\", \"url\": \"$RULESET_GEOSITE_CN\", \"download_detour\": \"direct\" }"

    # 构造 route 规则
    local wg_rules=""
    for g in $FOREIGN_GROUPS; do
        wg_rules+="        { \"rule_set\": \"geosite-${g}\", \"outbound\": \"wg-ep\" },
"
    done
    local domain_json=""
    for d in $FOREIGN_DOMAINS; do domain_json+="\"${d}\", "; done
    domain_json="${domain_json%, }"
    wg_rules+="        { \"domain\": [ ${domain_json} ], \"outbound\": \"wg-ep\" },
"
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
      { "rule_set": [ "geosite-cn", "geoip-cn" ], "server": "dns-direct" }
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
      "address": [ "${wg_ip}" ],
      "private_key": "${privkey}",
      "peers": [
        {
          "address": "${SERVER_HOST}",
          "port": ${SERVER_PORT},
          "public_key": "${SERVER_PUBKEY}",
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
    info "✓ 配置已生成: $SB_CONF"
    info "  代理端口: socks5+http 0.0.0.0:1080 (mixed 模式, 不碰路由表)"
}

#===============================================================
# systemd service
#===============================================================
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
    info "✓ systemd --user service: $svc_path"
    echo ""
    echo "  启用:  systemctl --user daemon-reload && systemctl --user enable --now ${SERVICE_NAME}"
    echo "  状态:  systemctl --user status ${SERVICE_NAME}"
    echo "  日志:  journalctl --user -u ${SERVICE_NAME} -f"
}

#===============================================================
# 主注册流程
#===============================================================
do_register() {
    check_token

    # 确定 client_id
    local client_id="$CLIENT_ID"
    if [[ -z "$client_id" ]]; then
        client_id=$(hostname 2>/dev/null || echo "client")
        # 简化 hostname (只取第一段, 去掉域名后缀)
        client_id=$(echo "$client_id" | cut -d. -f1 | tr '[:upper:]' '[:lower:]')
    fi
    info "client_id: $client_id"

    detect_platform
    detect_arch

    # 1. 克隆 gitee 仓库
    local repo_dir
    repo_dir=$(clone_repo)
    trap "rm -rf $repo_dir" EXIT

    local peers_dir="$repo_dir/peers"
    if [[ ! -d "$peers_dir" ]]; then
        error "peers/ 目录不存在。
请先在 lwtop 上运行: sudo bash server.sh --pre-gen <N>"
    fi

    # 2. 选择 peer
    local peer_file
    peer_file=$(select_peer "$peers_dir" "$client_id") || {
        error "没有可用的 peer (status=available)。
请在 lwtop 上运行: sudo bash server.sh --pre-gen <N>"
    }

    # 读取 peer 信息
    local peer_privkey peer_wg_ip peer_pubkey peer_orig_id peer_status
    peer_privkey=$(grep -oP '"private_key"\s*:\s*"\K[^"]+' "$peer_file")
    peer_wg_ip=$(grep -oP '"wg_ip"\s*:\s*"\K[^"]+' "$peer_file")
    peer_pubkey=$(grep -oP '"public_key"\s*:\s*"\K[^"]+' "$peer_file")
    peer_orig_id=$(grep -oP '"client_id"\s*:\s*"\K[^"]+' "$peer_file")
    peer_status=$(grep -oP '"status"\s*:\s*"\K[^"]+' "$peer_file")

    echo ""
    echo -e "${CYAN}═══════════════════════════════════════════${NC}"
    info "选中 peer: $peer_orig_id → $client_id"
    info "  wg_ip:       $peer_wg_ip"
    info "  public_key:  ${peer_pubkey:0:24}..."
    info "  status:      $peer_status"
    echo -e "${CYAN}═══════════════════════════════════════════${NC}"
    echo ""

    # dry-run 模式: 只展示, 不修改
    if [[ "$DRY_RUN" == true ]]; then
        info "[dry-run] 不修改任何文件"
        info "实际注册会:"
        info "  1. 标记该 peer 为 used 并 push 到 gitee"
        info "  2. 生成 sing-box config.json"
        info "  3. 下载并安装 sing-box"
        info "  4. 安装 systemd service"
        exit 0
    fi

    # 3. 校验 server 公钥 (防止 gitee 上的 SERVER_PUBKEY 被篡改)
    if [[ -f "$repo_dir/SERVER_PUBKEY" ]]; then
        local gitee_pubkey
        gitee_pubkey=$(cat "$repo_dir/SERVER_PUBKEY" | tr -d '[:space:]')
        if [[ -n "$gitee_pubkey" && "$gitee_pubkey" != "$SERVER_PUBKEY" ]]; then
            warn "gitee 上的 SERVER_PUBKEY ($gitee_pubkey) 与脚本内置 ($SERVER_PUBKEY) 不一致!"
            warn "使用脚本内置值 (更安全)"
        fi
    fi

    # 4. 标记 peer 为 used
    mark_peer_used "$peer_file" "$client_id" "$repo_dir"

    # 5. 生成配置
    mkdir -p "$SB_DIR" "$RULE_DIR"
    gen_config "$peer_privkey" "$peer_wg_ip"

    # 6. 安装 sing-box
    install_singbox

    # 7. 下载规则集
    download_rules

    # 8. 安装 systemd service
    gen_systemd_service

    # 9. 验证配置
    info "验证 sing-box 配置..."
    if "$SB_BIN" check -c "$SB_CONF" 2>&1; then
        info "✓ 配置验证通过"
    else
        warn "配置验证失败! 请检查 $SB_CONF"
    fi

    # 10. 完成提示
    echo ""
    echo -e "${YELLOW}══════════════════════════════════════════════════════════${NC}"
    echo -e "${YELLOW}  ⚠️  最后一步: 在 lwtop 上执行激活!${NC}"
    echo -e "${YELLOW}══════════════════════════════════════════════════════════${NC}"
    echo ""
    echo "  ssh lwtop"
    echo "  sudo bash server.sh --activate $client_id"
    echo ""
    echo -e "${GREEN}  激活后启动代理:${NC}"
    echo "    bash client.sh --test       # 10 秒测试"
    echo "    bash client.sh --start      # 启动"
    echo ""
    echo -e "${GREEN}  使用代理:${NC}"
    echo "    export http_proxy=http://127.0.0.1:1080"
    echo "    export https_proxy=http://127.0.0.1:1080"
    echo "    curl https://ifconfig.me    # 应显示 lwtop IP"
    echo ""
    info "注册完成! client_id=$client_id  wg_ip=$peer_wg_ip"
}

#===============================================================
# 入口
#===============================================================
case "$ACTION" in
    help)     show_help ;;
    list)     do_list ;;
    register) do_register ;;
    *)        show_help ;;
esac
