#!/usr/bin/env bash
#===============================================================
# gnp uninstall.sh — 卸载 gnp-client / gnp-server
#
# 流程:
#   1. 移除系统 PATH 中的链接
#   2. 可选: 删除安装目录 (~/.local/share/gnp/)
#   3. 可选: 删除 sing-box 数据目录 (~/.local/share/sing-box/)
#
# 用法:
#   bash uninstall.sh              # 移除系统链接
#   bash uninstall.sh --purge       # 移除链接 + 安装目录 + sing-box 数据
#   bash uninstall.sh --clean       # 移除链接 + 安装目录 (保留 sing-box 数据)
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

INSTALL_DIR="$HOME/.local/share/gnp"
INSTALL_BIN="$INSTALL_DIR/bin"
BINARIES=(gnp-client gnp-server)
OS="$(uname -s)"

detect_sys_bin() {
    case "$OS" in
        Darwin)
            if [ -w /usr/local/bin ]; then
                echo "/usr/local/bin"
            else
                echo "$HOME/.local/bin"
            fi
            ;;
        Linux)
            if [ -d "$HOME/.local/bin" ] || [ -w "$HOME" ]; then
                echo "$HOME/.local/bin"
            else
                echo "/usr/local/bin"
            fi
            ;;
        *) error "不支持的平台: $OS" ;;
    esac
}

main() {
    local purge=false
    local clean=false
    for arg in "$@"; do
        case "$arg" in
            --purge) purge=true ;;
            --clean) clean=true ;;
            -h|--help)
                echo "用法: bash uninstall.sh [--clean] [--purge]"
                echo "  (默认)    移除系统链接"
                echo "  --clean   移除链接 + 安装目录 (~/.local/share/gnp/)"
                echo "  --purge   移除链接 + 安装目录 + sing-box 数据 (~/.local/share/sing-box/)"
                exit 0
                ;;
        esac
    done

    local sys_bin
    sys_bin="$(detect_sys_bin)"

    # 1. 移除系统链接
    info "移除系统链接 ($sys_bin)..."
    for b in "${BINARIES[@]}"; do
        if [ -L "$sys_bin/$b" ]; then
            rm -f "$sys_bin/$b"
            info "  ✅ 已移除 $sys_bin/$b"
        else
            info "  (无) $sys_bin/$b"
        fi
    done

    # 2. 停止 sing-box 服务
    warn "停止 sing-box 服务..."
    "$INSTALL_BIN/gnp-client" stop 2>/dev/null || true

    # 3. 清理 cron (如果有)
    if crontab -l 2>/dev/null | grep -q "gnp-client"; then
        info "清理 cron 任务..."
        ( crontab -l 2>/dev/null | grep -v "gnp-client" ) | crontab -
        info "  ✅ cron 已清理"
    fi

    # 4. 可选: 删除安装目录
    if [ "$purge" = true ] || [ "$clean" = true ]; then
        if [ -d "$INSTALL_DIR" ]; then
            warn "删除安装目录: $INSTALL_DIR"
            rm -rf "$INSTALL_DIR"
            info "  ✅ 已删除 $INSTALL_DIR"
        else
            info "  安装目录不存在, 跳过"
        fi
    else
        info "保留安装目录: $INSTALL_DIR"
        info "如需删除: bash uninstall.sh --clean"
    fi

    # 5. --purge: 删除 sing-box 数据目录
    if [ "$purge" = true ]; then
        local sb_dir="$HOME/.local/share/sing-box"
        if [ -d "$sb_dir" ]; then
            warn "删除 sing-box 数据目录: $sb_dir"
            rm -rf "$sb_dir"
            info "  ✅ 已删除 $sb_dir"
        fi
    else
        info "保留 sing-box 数据: ~/.local/share/sing-box/"
        info "如需删除: bash uninstall.sh --purge"
    fi

    info "✅ 卸载完成"
}

main "$@"
