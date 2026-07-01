# 快速开始

Kiana Code 当前是 Claude Code 的 Rust 重构工作区。根 workspace 可以编译和测试，CLI、配置、session、doctor、release smoke、基础 TUI/REPL 路径已经接上；它还不是 reference 的完整替代品，remote live 服务和 reference 级 TUI 体验仍需要继续补齐。

## 1. 编译

```bash
cargo build --release -p kiana-entrypoints --bin kiana
./target/release/kiana --version
```

开发时也可以直接运行：

```bash
cargo run -p kiana-entrypoints --bin kiana -- --version
```

## 2. 配置 API Key

推荐先生成配置文件：

```bash
./target/release/kiana config init
./target/release/kiana login "sk-ant-xxx"
```

也可以使用环境变量：

```bash
export ANTHROPIC_API_KEY="sk-ant-xxx"
```

查看当前配置来源：

```bash
./target/release/kiana config status
```

## 3. 运行

无参数启动交互式 REPL：

```bash
./target/release/kiana
```

脚本、CI、管道环境使用非交互式 print 模式：

```bash
./target/release/kiana -p "summarize this repository"
```

完整终端 UI 入口：

```bash
./target/release/kiana tui
```

## 4. 常用检查

```bash
./target/release/kiana doctor
./target/release/kiana release
```

`doctor` 用来检查本地配置、工具、session/remote 入口状态。`release` 会列出发布前需要跑的 gate，并提示 optional remote live smoke 是否缺 token。

## 5. 本地 Session

```bash
./target/release/kiana session list
./target/release/kiana session show current
./target/release/kiana session reply current --record-only "记一条本地消息"
./target/release/kiana session compact current --dry-run
```

`--record-only` 只写入本地 transcript，不会调用模型。需要真实模型执行时，使用 CLI 的普通 `kiana session reply <id> <message>` 路径。

## 6. 开发验证

```bash
cargo fmt --all --check
cargo test --workspace --no-fail-fast
cargo build --release -p kiana-entrypoints --bin kiana
bash scripts/release-smoke.sh
```

如果只改命令层，可以先跑：

```bash
cargo test -p kiana-commands --no-fail-fast
```

## 当前可用边界

- 已接上：配置文件、环境变量、REPL/print mode、基础工具 runner、本地 slash command、session 管理、doctor、release smoke、部分 remote/bridge 诊断。
- 仍在补齐：reference 级 TUI 细节、完整 remote live 服务验证、Skills/Plugins 用户体验、更多协议 parity 和打包发布体验。
