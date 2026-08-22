# Kiana Code 安装指南

发布文档里的 clone URL 指向产品仓库 `https://github.com/kiana-project/kiana`（与 `Cargo.toml` `repository` 字段一致）。当前这个 checkout 的 origin 是 `https://github.com/shirosoralumie648/kianacode`；在本机开发时用实际 origin，不要把 fork URL 写进发布安装脚本。

## 快速安装

### 方式 1: 源码安装脚本（推荐）

```bash
# 脚本会克隆源码并用 cargo build --release 构建安装
curl -fsSL https://raw.githubusercontent.com/kiana-project/kiana/master/install.sh | bash
```

或在源码 checkout 中运行：

```bash
git clone https://github.com/kiana-project/kiana.git
cd kiana
./install.sh
```

### 方式 2: 使用 Makefile

```bash
git clone https://github.com/kiana-project/kiana.git
cd kiana
make install
```

### 方式 3: 手动编译

```bash
git clone https://github.com/kiana-project/kiana.git
cd kiana
cargo build --release -p kiana-entrypoints --bin kiana
cp target/release/kiana ~/.local/bin/
```

### 方式 4: 发布 tarball

```bash
tar -xzf kiana-<version>-<target>.tar.gz
cd kiana-<version>-<target>
INSTALL_DIR="$HOME/.local/bin" bash scripts/install-release-binary.sh
```

卸载 tarball 安装的二进制：

```bash
INSTALL_DIR="$HOME/.local/bin" bash scripts/install-release-binary.sh --uninstall
```

发布包交付前可以运行生命周期 smoke，验证 checksum、安装、重复安装、回滚恢复和卸载路径：

```bash
DIST_DIR="/path/to/dist" bash scripts/package-lifecycle-smoke.sh
```

## 系统要求

### 最低要求
- **操作系统**: Linux, macOS, Windows (WSL)
- **架构**: x86_64 或 aarch64
- **内存**: 512MB RAM
- **磁盘**: 100MB 可用空间

### 从源码编译
- **Rust**: 1.96+ (通过 [rustup](https://rustup.rs/) 安装；当前 `Cargo.lock` 使用 lockfile v4)
- **构建工具**:
  - Linux: `build-essential`, `libssl-dev`, `pkg-config`
  - macOS: Xcode Command Line Tools
  - Windows: MSVC 或 MinGW

默认构建不会编译 `kiana-computer-mcp` 的 native 桌面控制后端，因此 `cargo test --workspace` 不需要 X11/Wayland/PipeWire 开发库。需要真实截图、鼠标键盘输入或剪贴板 MCP 工具时，用主二进制的 `native-computer-use` feature 构建 `kiana`。

### 平台特定依赖

#### Linux
```bash
# Ubuntu/Debian
sudo apt install build-essential pkg-config libssl-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config openssl-devel
```

启用电脑控制 MCP native 后端时，还需要桌面开发库：

```bash
# Ubuntu/Debian
sudo apt install libxcb1-dev libxcb-randr0-dev libxcb-xtest0-dev \
                 libxcb-xinerama0-dev libxcb-shape0-dev libxcb-xkb-dev \
                 libwayland-dev libpipewire-0.3-dev libdbus-1-dev

# 构建带 native 电脑控制 MCP 的主二进制
cargo build -p kiana-entrypoints --bin kiana --features native-computer-use

# 启动电脑控制 MCP stdio server
./target/debug/kiana computer-mcp

# 验证 native 后端库和主入口 feature
cargo test -p kiana-computer-mcp --features native
cargo test -p kiana-entrypoints --features native-computer-use cli::tests::computer_mcp_stdio_route_accepts_command_and_legacy_flags
```

#### macOS
```bash
# 需要 Xcode Command Line Tools
xcode-select --install
```

#### Windows
WSL2 环境，按 Linux 依赖安装。

## 配置

### 1. API Key

设置 Anthropic API key（必需）：

```bash
export ANTHROPIC_API_KEY="your-api-key-here"
```

永久设置（添加到 `~/.bashrc` 或 `~/.zshrc`）：

```bash
echo 'export ANTHROPIC_API_KEY="your-api-key-here"' >> ~/.bashrc
```

### 2. 配置文件（可选）

创建 `~/.kiana/config.toml`：

```toml
[api]
key = "your-api-key"
model = "claude-sonnet-4-6"
max_tokens = 8192

[ui]
theme = "dark"
stream = true

[tools]
enabled = ["read", "write", "bash", "edit"]
```

### 3. 初始化配置文件

```bash
kiana config init
kiana login "your-api-key-here"
```

## 验证安装

```bash
# 检查版本
kiana --version

# 运行
kiana

# 脚本/CI/管道中使用非交互式 print 模式
kiana -p "hello"

# 检查远程 session 配置
kiana remote-session status

# 连接远程 session（需要 session/org/token）
KIANA_REMOTE_ACCESS_TOKEN=<token> kiana remote-session listen \
  --session-id <id> --org-uuid <uuid> --permission-mode deny
```

无参数 `kiana` 会启动交互式 REPL，需要真实 stdin/stdout 终端；脚本、CI 或管道环境请使用 `kiana -p <prompt>`，完整 TUI 使用 `kiana tui`。

源码 checkout 的发布前验收：

```bash
make release-smoke
```

该 gate 会在临时 `INSTALL_DIR` 中执行 `make install`，再验证已安装的 `kiana --version` 和 `kiana doctor`。

有真实远程服务 token 时，可额外验证 CCR v2 code-session 连通性：

```bash
KIANA_REMOTE_ACCESS_TOKEN=<token> make live-smoke
```

## 故障排除

### 问题: `kiana: command not found`

**解决**: 确保 `~/.local/bin` 在 PATH 中

```bash
# 添加到 ~/.bashrc 或 ~/.zshrc
export PATH="$PATH:$HOME/.local/bin"

# 重新加载配置
source ~/.bashrc
```

### 问题: 编译错误 `linker 'cc' not found`

**解决**: 安装 C 编译器

```bash
# Ubuntu/Debian
sudo apt install build-essential

# macOS
xcode-select --install
```

### 问题: `error: failed to run custom build command for openssl-sys`

**解决**: 安装 OpenSSL 开发库

```bash
# Ubuntu/Debian
sudo apt install libssl-dev pkg-config

# macOS (通过 Homebrew)
brew install openssl
export OPENSSL_DIR=$(brew --prefix openssl)
```

### 问题: computer-use native 后端缺少系统库（Linux）

**表现**: 使用 `--features native-computer-use` 构建 `kiana`，或使用 `kiana-computer-mcp --features native` 测试后端库时，出现 `pkg-config`、`wayland-client`、`xcb`、`pipewire` 等错误。

**解决**: 安装桌面开发库，或者不启用 `native-computer-use` feature。

```bash
sudo apt install pkg-config libxcb1-dev libwayland-dev \
                 libpipewire-0.3-dev libdbus-1-dev
```

### 问题: API key 未设置

**错误**: `Error: ANTHROPIC_API_KEY not found`

**解决**:
```bash
export ANTHROPIC_API_KEY="your-key"
```

### 问题: 权限错误

**解决**: 确保安装目录有写权限

```bash
chmod +x ~/.local/bin/kiana
```

### 问题: 源码安装失败

**解决**: 确认 Rust/cargo 和 git 可用，然后重新从源码安装。

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 编译安装
git clone https://github.com/kiana-project/kiana.git
cd kiana
make install
```

## 升级和回滚

升级前请先阅读 [CHANGELOG.md](CHANGELOG.md) 和 [UPGRADE.md](UPGRADE.md)。
商业环境应先在 staging 位置验证 `kiana --version`、`kiana doctor` 和
`kiana model list --json`，再替换生产二进制。回滚时保留上一版 binary 和
checksum，按 [UPGRADE.md](UPGRADE.md) 中的步骤恢复。

## 卸载

```bash
rm ~/.local/bin/kiana
rm -rf ~/.kiana  # 可选：删除配置、hooks、permissions、plugins 和本地状态
```

## 下一步

- 查看 [README.md](README.md) 了解功能
- 阅读 [QUICKSTART.md](QUICKSTART.md) 快速入门
- 查看 [USAGE.md](USAGE.md) 使用示例

## 获取帮助

- GitHub Issues: https://github.com/kiana-project/kiana/issues
- 文档: https://github.com/kiana-project/kiana
