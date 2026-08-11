#!/usr/bin/env bash
#===============================================================
# gnp uninstall.sh — 卸载 gnp-client / gnp-server
#
# 流程:
#   1. 移除系统 PATH 中的软链 (Mac: /usr/local/bin, Linux: ~/.local/bin)
#   2. 保留 $PROJECT/bin/ 构建产物 (可选 --clean 删除)
#
# 用法:
#   bash uninstall.sh            # 移除系统软链
#   bash uninstall.sh --clean    # 移除系统软链 + 删除 bin/ 构建产物
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$PROJECT_DIR/bin"
BINARIES=(gnp-client gnp-server)
OS="$(uname -s)"

# 检测系统 bin 目录 (与 install.sh 一致)
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
    local clean=false
    for arg in "$@"; do
        case "$arg" in
            --clean) clean=true ;;
            -h|--help)
                echo "用法: bash uninstall.sh [--clean]"
                echo "  (默认)    移除系统 PATH 软链"
                echo "  --clean   移除软链 + 删除 bin/ 构建产物"
                exit 0
                ;;
        esac
    done

    local sys_bin
    sys_bin="$(detect_sys_bin)"

    # 1. 移除系统软链
    info "移除系统软链 ($sys_bin)..."
    for b in "${BINARIES[@]}"; do
        if [ -L "$sys_bin/$b" ]; then
            rm -f "$sys_bin/$b"
            info "  ✅ 已移除 $sys_bin/$b"
        else
            info "  (无) $sys_bin/$b"
        fi
    done

    # 2. 可选: 删除 bin/ 构建产物
    if [ "$clean" = true ]; then
        warn "删除 bin/ 构建产物..."
        for b in "${BINARIES[@]}"; do
            rm -f "$BIN_DIR/$b"
            info "  ✅ 已删除 $BIN_DIR/$b"
        done
        # 若 bin/ 空则删除目录
        rmdir "$BIN_DIR" 2>/dev/null && info "  ✅ 已删除 bin/ 目录" || true
    else
        info "保留 $BIN_DIR/ 构建产物 (下次 install 无需重新构建)"
        info "如需删除: bash uninstall.sh --clean"
    fi

    info "✅ 卸载完成"
}

main "$@"