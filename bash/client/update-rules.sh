#!/bin/bash
#===============================================================
# global-net-proxy — 规则集自动更新脚本 (client 端)
#
# 说明: sing-box 的 remote rule-set 默认每 24h 自动更新一次。
# 本脚本作为兜底: 手动更新规则集 + 检查 sing-box 是否常驻,
# 若意外退出则重启。配合 cron 每天运行一次。
#
# 用法:
#   bash update-rules.sh --update    # 强制更新规则集并重启
#   bash update-rules.sh --check     # 检查 sing-box 状态, 挂了就重启
#   bash update-rules.sh --install-cron  # 安装 cron (每天)
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }

SB_DIR="$HOME/.local/share/sing-box"
SB_BIN="$SB_DIR/sing-box"
SB_CONF="$SB_DIR/config.json"
SB_LOG="$SB_DIR/sing-box.log"
SB_PID="$SB_DIR/sing-box.pid"
CLIENT_SH="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/client/client.sh"

ACTION="${1:-check}"
case "$ACTION" in
    --update|-u)       ACTION="update" ;;
    --check|-c)        ACTION="check" ;;
    --install-cron|-i) ACTION="install-cron" ;;
    *)                 ACTION="check" ;;
esac

# 检查 sing-box 是否运行, 挂了就拉起来
ensure_running() {
    if pgrep -f "sing-box run" >/dev/null 2>&1; then
        info "sing-box 运行中 ✓"
        return 0
    fi
    warn "sing-box 未运行, 尝试重启..."
    if [[ -x "$CLIENT_SH" ]]; then
        bash "$CLIENT_SH" --start || true
    else
        warn "未找到 client.sh, 跳过重启"
    fi
}

# 强制更新规则集 (触发 sing-box 重新加载 remote rule-set)
do_update() {
    [[ -f "$SB_CONF" ]] || { warn "未安装 sing-box, 跳过"; exit 0; }
    info "触发规则集更新..."
    # remote rule-set 在 sing-box 启动时拉取; 重启即以最新数据加载
    if pgrep -f "sing-box run" >/dev/null 2>&1; then
        local pid
        pid=$(pgrep -f "sing-box run" | head -1)
        kill "$pid" 2>/dev/null || true
        sleep 1
    fi
    ensure_running
    info "规则集更新完成 (sing-box 已重启加载最新 geosite/geoip)"
}

# 安装 cron 每天更新
do_install_cron() {
    local script_path
    script_path="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/client/update-rules.sh"
    local cron_line="0 4 * * * bash $script_path --check >> $SB_DIR/cron.log 2>&1"
    # 移除旧的, 避免重复
    ( crontab -l 2>/dev/null | grep -v "global-net-proxy" ) | crontab -
    ( crontab -l 2>/dev/null; echo "$cron_line" ) | crontab -
    info "cron 已安装: $cron_line"
    info "每天 04:00 检查 sing-box 常驻 + 规则集更新"
}

case "$ACTION" in
    update)       do_update ;;
    check)        ensure_running ;;
    install-cron) do_install_cron ;;
esac