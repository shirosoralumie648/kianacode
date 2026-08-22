# 快速开始

这份指南只覆盖当前 master 上已经接线的入口。Kiana 仍在控制平面迁移中，不是 Claude Code 的完整替代品，也不是 `reference/` 能力的完成统计。

## 1. 编译

```bash
cargo build --release -p kiana-entrypoints --bin kiana
./target/release/kiana --version
./target/release/kiana --help
```

开发时也可以：

```bash
cargo run -p kiana-entrypoints --bin kiana -- --version
```

当前 workspace `rust-version` 为 **1.96**。

## 2. 配置 API Key

推荐先生成配置文件：

```bash
./target/release/kiana config init
./target/release/kiana login "sk-ant-xxx"
./target/release/kiana config status
```

也可以使用环境变量：

```bash
# Anthropic
export ANTHROPIC_API_KEY="sk-ant-xxx"

# OpenAI-compatible
export KIANA_PROVIDER="openai-compatible"
export KIANA_OPENAI_API_KEY="sk-..."
export KIANA_OPENAI_BASE_URL="https://api.openai.com/v1"

# Ollama
export KIANA_PROVIDER="ollama"
export KIANA_OLLAMA_BASE_URL="http://localhost:11434"
export KIANA_OLLAMA_MODEL="llama3.1"
```

完整配置见 [CONFIG.md](CONFIG.md)。

## 3. 运行

无参数启动交互式 REPL（需要真实终端）：

```bash
./target/release/kiana
```

脚本、CI、管道使用非交互式 print 模式：

```bash
./target/release/kiana -p "summarize this repository"
```

产品 TUI：

```bash
./target/release/kiana tui
```

通过 owned harness 执行一轮 prompt：

```bash
./target/release/kiana run --json "summarize README.md"
```

查看控制平面迁移状态：

```bash
./target/release/kiana architecture status --json
```

## 4. Trust

新项目默认 `unknown`。项目级 skills、hooks、MCP、agents 和变更类能力 fail closed：

```bash
./target/release/kiana trust status
./target/release/kiana trust trust
```

## 5. 常用检查

```bash
./target/release/kiana doctor
./target/release/kiana release
./target/release/kiana session list
```

`doctor` 检查本地配置、工具和 session/remote 入口。`release` 列出发布前 gate，并提示 optional remote live smoke 是否缺 token。

## 6. 开发验证

```bash
cargo fmt --all --check
cargo test --workspace --locked --offline --no-fail-fast
bash scripts/schema-contract-smoke.sh
bash scripts/release-smoke.sh
```

如果只改命令层，可以先跑：

```bash
cargo test -p kiana-commands --no-fail-fast
```

## 当前可用边界

已接线：

- 配置文件、环境变量、REPL / print mode
- 产品 TUI（`kiana tui` / `kiana-screens`）
- 本地 slash command、session 管理、doctor、release smoke
- owned `KianaHarness`（`kiana run`）
- 部分 remote/bridge 诊断和 MCP server 入口

仍在补齐：

- reference 级 TUI 交互（crate 组件库 ≠ 产品界面已全部接线）
- 完整 remote live 服务验证
- plugins / scripting / VS Code extension（未进入 master workspace）
- 打包发布与商业就绪证据

下一步阅读 [USAGE.md](USAGE.md) 和 [docs/README.md](docs/README.md)。
