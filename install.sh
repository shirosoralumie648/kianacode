#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
REPO_URL="https://github.com/kiana-project/kiana"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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

if [[ "${1:-}" == "--uninstall" || "${1:-}" == "uninstall" ]]; then
    installed="$INSTALL_DIR/kiana${EXE_EXT}"
    if [[ -e "$installed" ]]; then
        rm -f "$installed"
        echo "Uninstalled $installed"
    else
        echo "Already absent $installed"
    fi
    desktop_dir="${KIANA_DESKTOP_DIR:-}"
    if [[ -z "$desktop_dir" && "${KIANA_SKIP_PATH_SETUP:-0}" != "1" ]]; then
        desktop_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
    fi
    if [[ -n "$desktop_dir" ]]; then
        rm -f "$desktop_dir/kiana.desktop"
    fi
    exit 0
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

copy_existing_bin() {
    local src="$1"
    if [[ ! -f "$src" ]]; then
        echo "❌ 找不到二进制: $src" >&2
        exit 1
    fi
    mkdir -p "$INSTALL_DIR"
    cp "$src" "$INSTALL_DIR/kiana${EXE_EXT}"
    chmod +x "$INSTALL_DIR/kiana${EXE_EXT}"
    echo "✅ 使用已有二进制: $src"
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

if [[ -n "${KIANA_BIN:-}" ]]; then
    copy_existing_bin "$KIANA_BIN"
else
    SOURCE_DIR="$(find_source_dir)"
    if [[ "${KIANA_SKIP_BUILD:-0}" == "1" ]]; then
        if [[ -x "$SOURCE_DIR/target/release/kiana${EXE_EXT}" ]]; then
            copy_existing_bin "$SOURCE_DIR/target/release/kiana${EXE_EXT}"
        elif [[ -x "$SOURCE_DIR/target/debug/kiana${EXE_EXT}" ]]; then
            copy_existing_bin "$SOURCE_DIR/target/debug/kiana${EXE_EXT}"
        else
            echo "❌ KIANA_SKIP_BUILD=1 但找不到 target/release/kiana 或 target/debug/kiana；设置 KIANA_BIN 或先编译" >&2
            exit 1
        fi
    else
        build_from_source "$SOURCE_DIR"
    fi
fi

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

install_desktop() {
    local template="$SCRIPT_DIR/contrib/kiana.desktop"
    local dest_dir="${KIANA_DESKTOP_DIR:-}"
    local dest
    if [[ ! -f "$template" ]]; then
        echo "⚠️  没有 contrib/kiana.desktop，跳过文件管理器入口"
        return 0
    fi
    if [[ -z "$dest_dir" ]]; then
        if [[ "${KIANA_SKIP_PATH_SETUP:-0}" == "1" ]]; then
            echo "✅ 跳过 desktop 入口（KIANA_SKIP_PATH_SETUP=1）"
            return 0
        fi
        dest_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
    fi
    mkdir -p "$dest_dir"
    dest="$dest_dir/kiana.desktop"
    python3 - "$template" "$dest" "$INSTALL_DIR/kiana${EXE_EXT}" <<'PY'
from pathlib import Path
import sys
src, dest, binary = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
text = src.read_text(encoding="utf8").replace("Exec=kiana --workdir %f", f"Exec={binary} --workdir %f")
text = text.replace("TryExec=kiana", f"TryExec={binary}")
dest.write_text(text, encoding="utf8")
PY
    echo "✅ 文件管理器入口: $dest"
}
install_desktop

installed="$INSTALL_DIR/kiana${EXE_EXT}"
doctor_output="$("$installed" doctor 2>&1 || true)"
run_help="$("$installed" run --help 2>&1 || true)"

if ! "$installed" --version &> /dev/null; then
    echo "❌ 安装验证失败: --version"
    exit 1
fi
if ! grep -q "Doctor" <<<"$doctor_output"; then
    echo "❌ 安装验证失败: doctor"
    exit 1
fi
if ! grep -q -- "--symposium" <<<"$run_help"; then
    echo "❌ 安装验证失败: kiana run --help 缺少 --symposium"
    exit 1
fi
if ! grep -q -- "--packet" <<<"$run_help"; then
    echo "❌ 安装验证失败: kiana run --help 缺少 --packet"
    exit 1
fi

echo ""
echo "🎉 安装成功！"
echo ""
echo "📝 真实可跑命令见 USER.md（trust / run / symposium / packet / review）。"
echo "   临时目录生命周期: bash scripts/v10-personal-lifecycle-smoke.sh"
echo "   cassette 黄金路径: bash scripts/harness-golden-smoke.sh"
echo "   卸载（幂等）: INSTALL_DIR=\"$INSTALL_DIR\" bash install.sh --uninstall"
    echo "   桌面壳（Electron，关窗可留托盘）: bash scripts/install-desktop.sh"
echo "   kiana tui 保持 park，不是产品路径。"
echo "   证明上限 local_behavior；live provider 不是安装完成条件。"
