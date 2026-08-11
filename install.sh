#!/usr/bin/env bash
#===============================================================
# gnp install.sh — 构建并安装 gnp-client / gnp-server 到系统 PATH
#
# 流程:
#   1. 检测 $PROJECT/bin/ 是否有构建产物 (gnp-client, gnp-server)
#   2. 没有则 cargo build --release → $PROJECT/bin/
#   3. 软链到 sys-bin-path (Mac: /usr/local/bin, Linux: ~/.local/bin 或 /usr/local/bin)
#
# 用法:
#   bash install.sh            # 构建 + 安装
#   bash install.sh --bin-only # 只构建到 $PROJECT/bin/ (不装到系统)
#
# 卸载: bash uninstall.sh
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# 项目根目录 (脚本所在目录的上一级)
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$PROJECT_DIR/bin"

# 目标二进制
BINARIES=(gnp-client gnp-server)

# 检测平台
OS="$(uname -s)"

# 选择系统 bin 目录
detect_sys_bin() {
    case "$OS" in
        Darwin)
            # macOS: 优先 /usr/local/bin (writable), 否则 ~/.local/bin
            if [ -w /usr/local/bin ]; then
                echo "/usr/local/bin"
            else
                echo "$HOME/.local/bin"
            fi
            ;;
        Linux)
            # Linux: 优先 ~/.local/bin (无需 root), 否则 /usr/local/bin
            if [ -d "$HOME/.local/bin" ] || [ -w "$HOME" ]; then
                echo "$HOME/.local/bin"
            else
                echo "/usr/local/bin"
            fi
            ;;
        *)
            error "不支持的平台: $OS"
            ;;
    esac
}

# 构建二进制到 $BIN_DIR
build() {
    info "构建发布二进制到 $BIN_DIR ..."
    mkdir -p "$BIN_DIR"
    cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml" 2>&1 | tail -5
    # 检查产物
    for b in "${BINARIES[@]}"; do
        if [ ! -f "$PROJECT_DIR/target/release/$b" ]; then
            error "构建失败: $b 未生成"
        fi
        # 复制到 bin/
        cp "$PROJECT_DIR/target/release/$b" "$BIN_DIR/$b"
        chmod +x "$BIN_DIR/$b"
        info "  ✅ $BIN_DIR/$b (v$( $BIN_DIR/$b --version | head -1))"
    done
}

# 安装到系统 PATH
install_sys() {
    local sys_bin
    sys_bin="$(detect_sys_bin)"
    info "安装到系统 bin: $sys_bin"
    mkdir -p "$sys_bin"

    for b in "${BINARIES[@]}"; do
        # 软链
        ln -sf "$BIN_DIR/$b" "$sys_bin/$b"
        info "  🔗 $sys_bin/$b → $BIN_DIR/$b"
    done

    # 确保 sys_bin 在 PATH
    case ":$PATH:" in
        *":$sys_bin:"*) ;;
        *) warn "⚠️  $sys_bin 不在 PATH 中, 请手动添加: export PATH=\"$sys_bin:\$PATH\"" ;;
    esac
}

# 主流程
main() {
    local bin_only=false
    for arg in "$@"; do
        case "$arg" in
            --bin-only) bin_only=true ;;
            -h|--help)
                echo "用法: bash install.sh [--bin-only]"
                echo "  (默认)       构建 + 安装到系统 PATH"
                echo "  --bin-only   只构建到 $PS4/bin/ (不装到系统)"
                exit 0
                ;;
        esac
    done

    # 检查 cargo
    if ! command -v cargo &>/dev/null; then
        error "未找到 cargo, 请先安装 Rust: https://rustup.rs"
    fi

    # 1. 检测 bin/ 是否有构建产物
    local need_build=false
    for b in "${BINARIES[@]}"; do
        if [ ! -f "$BIN_DIR/$b" ]; then
            need_build=true
            break
        fi
    done

    if [ "$need_build" = true ]; then
        warn "bin/ 缺少构建产物, 开始构建..."
        build
    else
        info "bin/ 已有构建产物, 跳过构建"
    fi

    # 2. 安装到系统
    if [ "$bin_only" = false ]; then
        install_sys
    fi

    info "✅ 安装完成!"
    info "  gnp-client: $(command -v gnp-client 2>/dev/null || echo "$sys_bin/gnp-client")"
    info "  gnp-server: $(command -v gnp-server 2>/dev/null || echo "$sys_bin/gnp-server")"
    info ""
    info "使用: gnp-client start | stop | status | wg | config | test"
    info "      gnp-server install | status | peers | add-peer | pregen | activate"
}

main "$@"