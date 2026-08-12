#!/usr/bin/env bash
#===============================================================
# gnp install.sh — 构建 + 安装所有依赖到安装目录
#
# 安装后 gnp-client/gnp-server 可独立工作, 不依赖 repo。
#
# 安装布局 (~/.local/share/gnp/):
#   bin/gnp-client     — client CLI (cargo build)
#   bin/gnp-server     — server CLI (cargo build)
#   lib/sing-box       — sing-box 二进制 (下载)
#
# sing-box 数据目录 (~/.local/share/sing-box/):
#   sing-box           — sing-box 二进制 (symlink → 安装目录)
#   config.json        — 配置 (运行时生成)
#   rules/             — 规则集 (运行时下载)
#
# 系统链接:
#   ~/.local/bin/gnp-client → 安装目录/bin/gnp-client
#   ~/.local/bin/gnp-server → 安装目录/bin/gnp-server
#
# 用法:
#   bash install.sh              # 构建 + 安装到系统 PATH
#   bash install.sh --bin-only    # 只构建到安装目录
#   bash install.sh --no-build   # 只安装 (不构建, 用已有产物)
#
# 卸载: bash uninstall.sh
#===============================================================
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# --- 常量 ---
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_DIR="$HOME/.local/share/gnp"
INSTALL_BIN="$INSTALL_DIR/bin"
SYS_BIN_DIR=""  # 稍后检测

# sing-box 下载信息
SB_VERSION="${SB_VERSION:-1.13.16}"
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64|amd64)  ARCH="amd64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    *) error "不支持的架构: $ARCH" ;;
esac

# 二进制列表
BINARIES=(gnp-client gnp-server)

# --- 参数解析 ---
NO_BUILD=false
BIN_ONLY=false
for arg in "$@"; do
    case "$arg" in
        --bin-only)  BIN_ONLY=true ;;
        --no-build)  NO_BUILD=true ;;
        -h|--help)
            echo "用法: bash install.sh [--bin-only] [--no-build]"
            echo "  (默认)       构建 + 安装到系统 PATH"
            echo "  --bin-only   只构建到安装目录 (不链接到 PATH)"
            echo "  --no-build   跳过构建, 用已有产物"
            exit 0
            ;;
    esac
done

# --- 平台检测 ---
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

# --- 构建 ---
build() {
    info "构建发布二进制..."
    cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml" 2>&1 | tail -5
    for b in "${BINARIES[@]}"; do
        if [ ! -f "$PROJECT_DIR/target/release/$b" ]; then
            error "构建失败: $b 未生成"
        fi
    done
    info "✅ 构建完成"
}

# --- 安装到安装目录 ---
install_to_dir() {
    mkdir -p "$INSTALL_BIN"

    # 1. 复制 Rust 二进制
    for b in "${BINARIES[@]}"; do
        cp "$PROJECT_DIR/target/release/$b" "$INSTALL_BIN/$b"
        chmod +x "$INSTALL_BIN/$b"
        info "  ✓ $INSTALL_BIN/$b"
    done

    # 2. 下载 sing-box 到安装目录
    local sb_target="$INSTALL_DIR/lib/sing-box"
    if [ -x "$sb_target" ]; then
        info "  ✓ sing-box v${SB_VERSION} 已存在: $sb_target"
    else
        mkdir -p "$INSTALL_DIR/lib"
        local sb_url=""
        case "$OS" in
            Darwin) sb_url="https://github.com/SagerNet/sing-box/releases/download/v${SB_VERSION}/sing-box-${SB_VERSION}-darwin-${ARCH}.tar.gz" ;;
            Linux)  sb_url="https://github.com/SagerNet/sing-box/releases/download/v${SB_VERSION}/sing-box-${SB_VERSION}-linux-${ARCH}.tar.gz" ;;
        esac
        if [ -z "$sb_url" ]; then
            warn "  跳过 sing-box 下载 (不支持的平台: $OS)"
        else
            info "  ⬇️  下载 sing-box v${SB_VERSION} ($OS/$ARCH)..."
            local tmp="$INSTALL_DIR/lib/.download"
            mkdir -p "$tmp"
            curl -fL --retry 3 -o "$tmp/sing-box.tar.gz" "$sb_url"
            tar -xzf "$tmp/sing-box.tar.gz" -C "$tmp"
            local found
            found=$(find "$tmp" -name 'sing-box' -type f | head -1)
            if [ -n "$found" ]; then
                mv "$found" "$sb_target"
                chmod +x "$sb_target"
                info "  ✓ sing-box 安装完成: $sb_target"
            else
                error "  sing-box 二进制未找到"
            fi
            rm -rf "$tmp"
        fi
    fi

    # 3. 创建 sing-box 运行目录 (数据目录, 存 config/rules/log)
    local sb_data="$HOME/.local/share/sing-box"
    mkdir -p "$sb_data/rules"

    # 4. 链接 sing-box 到数据目录 (gnp-core 期望的路径)
    local sb_data_bin="$sb_data/sing-box"
    if [ ! -e "$sb_data_bin" ]; then
        ln -sf "$sb_target" "$sb_data_bin"
        info "  ✓ $sb_data_bin → $sb_target"
    fi

    info "✅ 安装目录: $INSTALL_DIR"
}

# --- 安装到系统 PATH ---
install_sys() {
    SYS_BIN_DIR="$(detect_sys_bin)"
    info "安装到系统 PATH: $SYS_BIN_DIR"
    mkdir -p "$SYS_BIN_DIR"

    for b in "${BINARIES[@]}"; do
        # 用 symlink 指向安装目录
        ln -sf "$INSTALL_BIN/$b" "$SYS_BIN_DIR/$b"
        info "  🔗 $SYS_BIN_DIR/$b → $INSTALL_BIN/$b"
    done

    # 确保 PATH
    case ":$PATH:" in
        *":$SYS_BIN_DIR:"*) ;;
        *) warn "⚠️  $SYS_BIN_DIR 不在 PATH 中, 请手动添加: export PATH=\"$SYS_BIN_DIR:\$PATH\"" ;;
    esac
}

# --- 主流程 ---
main() {
    # 1. 检查 cargo
    if [ "$NO_BUILD" = false ]; then
        if ! command -v cargo &>/dev/null; then
            error "未找到 cargo, 请先安装 Rust: https://rustup.rs"
        fi
        build
    else
        # 验证产物存在
        for b in "${BINARIES[@]}"; do
            if [ ! -f "$PROJECT_DIR/target/release/$b" ]; then
                error "产物不存在: $b (--no-build 但 target/release/$b 不存在)"
            fi
        done
        info "跳过构建, 使用已有产物"
    fi

    # 2. 安装到安装目录
    install_to_dir

    # 3. 安装到系统 PATH
    if [ "$BIN_ONLY" = false ]; then
        install_sys
    fi

    info "✅ 安装完成!"
    info "  gnp-client: ${SYS_BIN_DIR:-$INSTALL_BIN}/gnp-client"
    info "  gnp-server: ${SYS_BIN_DIR:-$INSTALL_BIN}/gnp-server"
    info ""
    info "使用:"
    info "  gnp-client start | stop | status | hy2 | config | test | install"
    info "  gnp-client register --list               # 查看 peer 池"
    info "  gnp-client register --client-id myname    # 自动注册"
    info "  gnp-client update-rules --check          # 检查 sing-box 守护"
    info "  gnp-client cleanup                       # 应急清理"
    info "  gnp-client recover                       # 断网恢复"
    info ""
    info "  gnp-server install | status | peers | add-peer | pregen | activate"
}

main "$@"
