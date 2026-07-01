#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
REPO_URL="https://github.com/kiana-project/kiana"

echo "🎉 Kiana Code 安装脚本"
echo "========================="

# 检测操作系统
detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "macos" ;;
        MSYS*|MINGW*|CYGWIN*) echo "windows" ;;
        *) echo "unknown" ;;
    esac
}

# 检测架构
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) echo "unknown" ;;
    esac
}

OS=$(detect_os)
ARCH=$(detect_arch)
EXE_EXT=""
if [[ "$OS" == "windows" ]]; then
    EXE_EXT=".exe"
fi

if [[ "$OS" == "unknown" ]] || [[ "$ARCH" == "unknown" ]]; then
    echo "❌ 不支持的系统: $OS ($ARCH)"
    exit 1
fi

echo "📦 检测到系统: $OS ($ARCH)"

find_source_dir() {
    if [[ -f "Cargo.toml" && -d "kiana-entrypoints" ]]; then
        pwd
        return 0
    fi

    if ! command -v git &> /dev/null; then
        echo "❌ 当前目录不是 kiana 源码树，且系统没有 git，无法克隆源码进行安装" >&2
        exit 1
    fi

    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT
    echo "📥 克隆源码到临时目录..." >&2
    git clone --depth 1 "$REPO_URL" "$TMP_DIR/kiana" >/dev/null
    printf '%s\n' "$TMP_DIR/kiana"
}

build_from_source() {
    local source_dir="$1"
    echo "🔨 从源码编译..."

    if ! command -v cargo &> /dev/null; then
        echo "❌ 需要安装 Rust/cargo 才能从源码安装"
        echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi

    (cd "$source_dir" && cargo build --release -p kiana-entrypoints --bin kiana)

    mkdir -p "$INSTALL_DIR"
    cp "$source_dir/target/release/kiana${EXE_EXT}" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/kiana${EXE_EXT}"

    echo "✅ 编译完成"
}

# 安装
SOURCE_DIR="$(find_source_dir)"
build_from_source "$SOURCE_DIR"

# 设置 PATH
setup_path() {
    local shell_rc=""

    if [[ -n "${BASH_VERSION:-}" ]]; then
        shell_rc="$HOME/.bashrc"
    elif [[ -n "${ZSH_VERSION:-}" ]]; then
        shell_rc="$HOME/.zshrc"
    else
        shell_rc="$HOME/.profile"
    fi

    if [[ ! ":$PATH:" == *":$INSTALL_DIR:"* ]]; then
        echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$shell_rc"
        echo "✅ 已添加到 PATH ($shell_rc)"
        export PATH="$PATH:$INSTALL_DIR"
    else
        echo "✅ PATH 已配置"
    fi
}

if [[ "${KIANA_SKIP_PATH_SETUP:-0}" == "1" ]]; then
    echo "✅ 跳过 PATH 配置（KIANA_SKIP_PATH_SETUP=1）"
else
    setup_path
fi

# 验证安装
doctor_output="$("$INSTALL_DIR/kiana${EXE_EXT}" doctor 2>&1 || true)"
if "$INSTALL_DIR/kiana${EXE_EXT}" --version &> /dev/null && grep -q "Doctor" <<<"$doctor_output"; then
    echo ""
    echo "🎉 安装成功！"
    echo ""
    echo "📝 下一步:"
    echo "   1. 初始化配置文件（可选）:"
    echo "      kiana config init"
    echo ""
    echo "   2. 设置 API key（二选一）:"
    echo "      export ANTHROPIC_API_KEY=\"your-key\""
    echo "      kiana login \"your-key\""
    echo ""
    echo "   3. 启动 Kiana:"
    echo "      kiana"
    echo ""
    echo "💡 首次使用？查看快速入门:"
    echo "   https://github.com/kiana-project/kiana#quickstart"
else
    echo "❌ 安装验证失败"
    exit 1
fi
