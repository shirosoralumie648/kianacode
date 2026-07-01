#!/bin/bash
set -e

echo "🚀 Kiana Code - 一键运行脚本"
echo

CONFIG_DIR="$HOME/.kiana"
CONFIG_FILE="$CONFIG_DIR/config.toml"

# 配置函数
configure() {
    mkdir -p "$CONFIG_DIR"

    echo "📝 配置 Kiana Code"
    echo

    # API Key
    read -p "Anthropic API Key: " api_key
    if [ -z "$api_key" ]; then
        echo "❌ API Key 不能为空"
        exit 1
    fi

    # Base URL（可选）
    echo
    echo "Base URL (留空使用默认 https://api.anthropic.com):"
    read -p "> " base_url

    # 写入配置
    cat > "$CONFIG_FILE" <<EOF
# Kiana Code 配置文件
api_key = "$api_key"
EOF

    if [ -n "$base_url" ]; then
        echo "base_url = \"$base_url\"" >> "$CONFIG_FILE"
    fi

    echo
    echo "✅ 配置已保存到 $CONFIG_FILE"
    echo
}

# 检查参数
if [ "$1" = "config" ] || [ "$1" = "configure" ]; then
    configure
    exit 0
fi

# 检查配置
if [ -z "$ANTHROPIC_API_KEY" ] && [ ! -f "$CONFIG_FILE" ]; then
    echo "⚠️  首次使用需要配置 API Key"
    echo
    read -p "是否现在配置？(y/n): " answer
    if [ "$answer" = "y" ] || [ "$answer" = "Y" ]; then
        configure
    else
        echo
        echo "请手动配置："
        echo "  1. 设置环境变量："
        echo "     export ANTHROPIC_API_KEY=\"your-key\""
        echo
        echo "  2. 或运行配置命令："
        echo "     ./run.sh config"
        exit 1
    fi
fi

# 显示配置状态
if [ -f "$CONFIG_FILE" ]; then
    echo "✓ 配置文件：$CONFIG_FILE"
fi
if [ -n "$ANTHROPIC_API_KEY" ]; then
    echo "✓ 环境变量：ANTHROPIC_API_KEY 已设置"
fi
echo

# 检查是否已编译
if [ ! -f "target/release/kiana" ]; then
    echo "📦 首次运行，正在编译（发布版）..."
    cargo build --release --bin kiana
    echo "✓ 编译完成"
    echo
fi

echo "🎯 启动 Kiana Code..."
echo "提示：按 Ctrl+C 退出"
echo
./target/release/kiana
