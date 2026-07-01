# Kiana Code 使用指南

本文只描述当前 Rust workspace 已接上的真实入口。历史重构计划和阶段总结请看 `ROADMAP.md` / `REWRITE-REPORT.md`，不要把它们当作当前可执行说明。

## 启动方式

```bash
# 构建发布版
cargo build --release -p kiana-entrypoints --bin kiana

# 交互式 REPL，需要真实终端
./target/release/kiana

# 非交互式 print 模式，适合脚本/CI/管道
./target/release/kiana -p "explain this directory"

# TUI
./target/release/kiana tui
```

开发模式：

Inside the TUI, `/history` opens the persisted prompt history picker. History is
stored under `KIANA_HOME/tui-history.jsonl`, deduped newest-first, searchable, and
restores the selected prompt as a draft.

```bash
cargo run -p kiana-entrypoints --bin kiana -- -p "hello"
```

## 配置

```bash
kiana config init
kiana login "sk-ant-xxx"
kiana config status
kiana auth status --json
```

也可以直接写入配置项：

```bash
kiana config set api_key "sk-ant-xxx"
kiana config set model "claude-sonnet-4-6"
kiana config get api_key
kiana model list
kiana model list --json
kiana model smoke --json
kiana license status --json
```

环境变量优先级高于配置文件：

```bash
export ANTHROPIC_API_KEY="sk-ant-xxx"
export ANTHROPIC_BASE_URL="https://api.anthropic.com"
```

OpenAI-compatible 文本 provider：

```bash
export KIANA_PROVIDER="openai-compatible"
export KIANA_OPENAI_API_KEY="sk-xxx"
export KIANA_OPENAI_BASE_URL="https://api.openai.com/v1"
kiana -p "summarize README.md"
```

这个 provider 支持 OpenAI Chat Completions 文本与 function-style tools；`kiana model list --json` 会显示 `supports_tools=true`。如果只想走纯文本路径，可以显式传 `--tools ""`。

本地 Ollama 文本 provider：

```bash
export KIANA_PROVIDER="ollama"
export KIANA_OLLAMA_BASE_URL="http://localhost:11434"
export KIANA_OLLAMA_MODEL="llama3.1"
kiana -p "summarize README.md"
```

这个 provider 当前只声明 text-only `/api/chat` 能力，不需要 API key；显式带工具运行会在请求前失败。

Provider smoke report 默认只执行无网络 fake provider，并把 Anthropic、OpenAI-compatible、Ollama 标为 skipped，适合 CI/release gate：

```bash
kiana model smoke --json
```

真实 provider smoke 需要显式 opt-in：

```bash
KIANA_PROVIDER_SMOKE_LIVE=1 kiana model smoke --json
# 或
kiana model smoke --live --json
```

## 基础对话

Enterprise license readiness is local and offline by default. It reports whether
account, plan, entitlements, support contact, and managed policy inputs are
configured without printing the raw license key:

```bash
export KIANA_LICENSE_KEY="kiana-enterprise-..."
export KIANA_ENTERPRISE_ACCOUNT_ID="acct_..."
export KIANA_LICENSE_PLAN="enterprise"
export KIANA_LICENSE_ENTITLEMENTS="managed-policy,offline"
kiana license status --json
```

```bash
kiana -p "summarize README.md"
```

交互式模式下可以连续对话。当前 runner 已接入 Read、Write、Edit、Glob、Grep、Bash、Agent、Todo、MCP 等工具路径，但 reference 级权限、展示和边界行为仍在继续核对。

## 权限

```bash
kiana permissions status
kiana permissions profile read-only
kiana permissions profile workspace
kiana permissions profile full
kiana permissions profile ask
kiana permissions profile plan
kiana --permission-profile read-only -p "summarize this repo"
```

`read-only`/`plan` 会阻止默认的变更类工具，`ask` 会在可交互终端请求确认，`workspace` 使用默认工作区策略，`full` 对应跳过权限提示的高风险模式。

## Session 管理

```bash
kiana session list
kiana session status
kiana session current
kiana session show current
kiana session reply current --record-only "本地补充一条消息"
kiana session rename current "调试记录"
kiana session tag current smoke
kiana session fork current
kiana session export current --format text session.txt
kiana session compact current --dry-run
kiana session delete <session_id>
```

本地 slash command 和 TUI 使用同一套 session command。`--record-only` 不调用模型，只更新本地 session；需要模型回复时使用：

```bash
kiana session reply <session_id> "继续解释刚才的问题"
```

## Doctor 和 Release

```bash
kiana doctor
kiana release
```

`doctor` 会报告本地配置、bridge、remote-session、live smoke token 等状态。`ANTHROPIC_AUTH_TOKEN=sk-*` 会被标记为 remote live smoke 的 token 误用，因为它是 Anthropic API key，不是 Claude/remote bearer token。

`release` 会列出发布前 gate：

```bash
cargo fmt --all --check
cargo test --workspace --no-fail-fast
cargo build --release -p kiana-entrypoints --bin kiana
./target/release/kiana --version
./target/release/kiana doctor
bash scripts/release-smoke.sh
```

如果要跑 optional remote live smoke，需要设置：

```bash
export KIANA_REMOTE_ACCESS_TOKEN="<remote bearer token>"
# 或
export CLAUDE_ACCESS_TOKEN="<remote bearer token>"
```

## Remote Session

本地入口已经存在，但真实 live smoke 依赖有效的 remote session、org uuid 和 remote bearer token：

```bash
kiana remote-session status
KIANA_REMOTE_ACCESS_TOKEN=<token> kiana remote-session listen \
  --session-id <id> --org-uuid <uuid> --permission-mode deny
kiana remote-session code-session smoke --json
```

没有真实 token 时，优先使用 `kiana doctor` / `kiana release` 看诊断，不要把 `ANTHROPIC_AUTH_TOKEN=sk-*` 当作 remote token。

## TUI

```bash
kiana tui
```

TUI 当前能走本地 slash command、doctor、session resume、record-only reply、compact、fork 等路径。它仍是重构中的骨架，目标是继续向 reference 的完整交互体验靠齐。

## 常见问题

### `kiana` 在脚本里卡住

无参数 `kiana` 会进入交互式 REPL。脚本、CI、管道里请用：

```bash
kiana -p "your prompt"
```

### 找不到配置

```bash
kiana config init
kiana login "sk-ant-xxx"
kiana config status
```

### remote live smoke 提示 token 无效

如果看到 `ANTHROPIC_AUTH_TOKEN is sk-* API key`，请设置 `KIANA_REMOTE_ACCESS_TOKEN` 或 `CLAUDE_ACCESS_TOKEN`。`ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN=sk-*` 只适合 Anthropic API，不适合 remote live smoke。

## 当前边界

- 可用：Rust workspace 编译、配置、REPL/print、基础工具 runner、本地 session、doctor、release smoke、部分 remote/bridge 诊断。
- 未完成：reference 级 TUI、完整 Skills/Plugins 体验、所有 remote live 场景、商业化安装发布和更细协议 parity。
