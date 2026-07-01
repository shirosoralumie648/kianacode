# Kiana Code

**Claude Code 的 Rust 重构工作区** - 当前可编译、可测试，仍在逐段补齐 `docs/reference-migration-roadmap.md` 中记录的 reference parity 缺口

## 当前状态

```bash
# 设置 API key
export ANTHROPIC_API_KEY="your-key-here"

# 运行
cargo run -p kiana-entrypoints --bin kiana

# 或使用发布版（更快）
cargo build --release -p kiana-entrypoints --bin kiana
./target/release/kiana

# 脚本/管道中使用非交互式 print 模式
./target/release/kiana -p "summarize this repository"
```

无参数 `kiana` 会启动交互式 REPL，需要真实 stdin/stdout 终端；脚本、CI 或管道环境请使用 `kiana -p <prompt>`，完整 TUI 使用 `kiana tui`。

当前根 workspace 已经具备 CLI、SDK session、工具执行、MCP、remote/bridge、TUI 骨架和 native shim 替换等模块，并有工作区测试覆盖。它还不能声明为完整替代 Claude Code reference；真实 remote 服务、完整 TUI 体验、商业化安装发布和更细的协议 parity 仍需要继续验证。

## ✨ 当前功能

### 已验证/正在推进
- **交互式 REPL** - 流式对话，保持上下文
- **工具系统** - AI 可以操作文件和执行命令
  - Read - 读取文件
  - Write - 写入文件
  - Bash - 执行命令
- **配置系统** - 支持环境变量和配置文件
- **API 集成** - Anthropic Messages API 主链路已接入，仍需继续对齐 reference 的边界行为
- **TUI 骨架** - 支持本地 slash command 路由、真实 doctor 诊断、session resume 和基础对话视图；resume 列表会优先限定到当前工作目录，`/session reply current --record-only ...` 和 `/session compact current ...` 会同步刷新当前转录，`/session fork current` 会切到 forked session 继续工作，不再只显示内部 JSON/命令报告，仍需继续补齐 reference 级交互体验
- **本地会话管理** - `kiana session list/status/show/reply --record-only/rename/tag/fork/delete/export/compact` 走同源 session command，列表会显示消息数、tag、更新时间和后续操作提示；`kiana session reply <id> <msg>` 默认输出 assistant 文本，`kiana --continue` 会优先继续当前工作目录最近的本地 SDK session
- **Bridge/CCR v2** - mock 服务下已覆盖 Session Ingress 与 CCR v2 worker transport 的关键闭环；CCR v2 收到远端 user event 会上报 `running`，permission lifecycle 会按 reference 上报 `requires_action`、`running`、`idle` worker state，并对连续同状态更新去重；stream-json 输入和 SDK URL bridge child loop 会按 reference 处理 `end_session`，bridge 父层和 child 层会响应 `mcp_status` 并返回 `mcpServers` 数组

### 仍在补齐
- UI 体验（ratatui）
- 更多工具（50+ 工具）
- Skills/Plugins 系统
- MCP 支持
- 打包发布

## 📖 使用示例

```
You: 帮我创建一个 hello.txt 文件
Assistant: 好的，我来创建...
[Tool: Write]
✓ 文件已创建

You: 读取这个文件
[Tool: Read]
文件内容：Hello, World!

You: 列出当前目录
[Tool: Bash]
hello.txt
kiana-bootstrap/
kiana-services/
...
```

## 📦 项目结构

23 个独立 crate 的 workspace：

### 核心基础设施
- **kiana-constants** - 应用常量和配置
- **kiana-types** - 类型定义和数据结构
- **kiana-bootstrap** - 初始化和会话状态
- **kiana-coordinator** - 多 agent 协调模式

### 任务和执行
- **kiana-tasks** - 后台任务执行系统
- **kiana-tools** - 50+ 工具实现（File、Bash、Agent 等）
- **kiana-query** - 查询引擎编排

### 服务层
- **kiana-services** - 核心服务（API、MCP、analytics）
- **kiana-remote** - 远程会话管理
- **kiana-bridge** - 远程集成层

### 功能扩展
- **kiana-skills** - 动态技能加载系统
- **kiana-plugins** - 插件系统

### UI 层（终端）
- **kiana-components** - ratatui UI 组件
- **kiana-ink** - 终端 UI 框架
- **kiana-screens** - 顶层屏幕（REPL、diagnostics）

### 命令系统
- **kiana-commands** - 90+ slash command 实现
- **kiana-entrypoints** - 程序入口点（CLI、SDK、MCP）

### 平台功能（替换原私有 shim）
- **kiana-computer-input** - 输入模拟（enigo + rdev）
- **kiana-computer-mcp** - 电脑控制 MCP server；默认构建只暴露状态工具，真实截图/输入/剪贴板后端需要主二进制的 `native-computer-use` feature
- **kiana-screen-capture** - 截图能力抽象
- **kiana-chrome-mcp** - 浏览器自动化（chromiumoxide）
- **kiana-color-diff** - 语法高亮 diff（syntect）
- **kiana-modifiers** - 修饰键查询（rdev）
- **kiana-url-handler** - URL scheme 处理

### 工具函数
- **kiana-utils** - 80+ 工具函数（auth、config、git、hooks）

## 🛠️ 技术栈

- **语言**: Rust 2021 edition
- **异步运行时**: Tokio
- **HTTP**: reqwest + axum
- **序列化**: serde + serde_json
- **CLI**: clap v4
- **TUI**: ratatui + crossterm + taffy
- **错误处理**: anyhow + thiserror
- **平台功能**: enigo, rdev, xcap, chromiumoxide（桌面 computer-use 后端为可选 `native-computer-use` feature）
- **语法高亮**: syntect
- **Diff**: similar

## ✨ 特性

### vs 原 TypeScript 版本的目标
- ✅ **100% 开源依赖** - 移除所有私有/native shim
- ✅ **跨平台优先** - macOS/Linux/Windows
- ✅ **类型安全** - Rust 类型系统
- ✅ **零 unsafe** - 除必要 FFI
- ✅ **性能优化** - 原生编译，零 GC 暂停
- ✅ **小体积** - 单一可执行文件

这些是重构目标和已推进方向，不等同于已经完成 reference 级行为替代。

### 架构改进
- 🏗️ **模块化 workspace** - 23 个独立 crate
- 🔄 **清晰依赖** - 显式 crate 边界
- 📦 **增量编译** - 只编译修改的 crate
- 🧪 **独立测试** - 每个 crate 可单独测试

## 📋 开发指南

### 构建
```bash
# 检查编译
cargo check --workspace

# 构建所有包
cargo build --workspace

# 构建带电脑控制 MCP native 后端的 kiana 二进制（需要系统桌面开发库）
cargo build -p kiana-entrypoints --bin kiana --features native-computer-use

# 启动电脑控制 MCP stdio server
./target/debug/kiana computer-mcp

# 检查远程 session 配置
./target/debug/kiana remote-session status

# 通过 Sessions API 列出、查看、重命名远程 session
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session list --org-uuid <uuid>
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session show <id> --org-uuid <uuid>
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session rename <id> "New title" --org-uuid <uuid>

# 通过 Sessions API 创建、归档远程 session
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session create \
  --org-uuid <uuid> --environment-id <env-id> --title "Remote task" \
  --permission-mode acceptEdits --git-url https://github.com/acme/widgets.git \
  --git-revision main --outcome-branch claude/task "fix the issue"
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session create \
  --org-uuid <uuid> --environment-id <env-id> --seed-bundle \
  --title "Seeded local task" "fix this local checkout"
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session archive <id> --org-uuid <uuid>

# 创建 CCR v2 env-less code session、获取 worker credentials、打印 SDK URL
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session code-session create \
  --title "Remote bridge" --tag ccr-mirror
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session code-session bridge \
  <cse_id> --trusted-device-token <token> --json
./target/debug/kiana remote-session code-session sdk-url <cse_id>
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session code-session hydrate \
  <cse_id> --trusted-device-token <token> --json
# kiana-remote 覆盖 CCR v2 /worker/*、SSE client_event、sequence resume 重连、
# liveness timeout 与 transient HTTP 写入 retry；
# kiana-bridge 在 use_code_sessions=true 时会走 CCR v2 worker transport，
# 并转发 stream-json 非终态输出、聚合 stream_event text_delta 为 full-so-far 快照；
# CCR v2 session loop 会用 100ms maintenance tick 独立 flush 到期 stream_event，
# client events 会按 reference 边界拆成最多 100 条/10MiB 的串行批次并做队列背压，
# internal events 会按 reference 边界拆成最多 100 条/10MiB、pending window 200 的串行批次，
# CCR v2 bridge 会自动写入 foreground user/assistant transcript internal events，
# 并支持 /worker/internal-events 与 subagents=true 的 cursor 分页读取，
# code-session hydrate 会把 foreground transcript internal events 写成本地 SDK session，
# 并把 subagent transcript internal events 按 agent 写入本地 JSONL，
# delivery updates 会按最多 64 条批量上报，并在 EOF/timeout/reconnect 前 flush。

# 列出、选择、创建默认远程运行环境
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session environments --org-uuid <uuid>
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session environments selected --org-uuid <uuid>
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session environments create-default "Default Cloud" --org-uuid <uuid>

# 打印远程 session WebSocket URL（离线检查）
./target/debug/kiana remote-session url --session-id <id> --org-uuid <uuid>

# 向已有远程 session 发送用户消息事件
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session send \
  --session-id <id> --org-uuid <uuid> "continue"

# 连接远程 session 并按 JSONL 输出消息
KIANA_REMOTE_ACCESS_TOKEN=<token> ./target/debug/kiana remote-session listen \
  --session-id <id> --org-uuid <uuid> --permission-mode deny
# listen 会透传 can_use_tool 权限请求，并对 initialize/unsupported control request 快速响应，
# 避免 Session Ingress 等待 control_response 超时；如果远程 WS 因 4003 关闭，
# 可通过 KIANA_REMOTE_REFRESH_COMMAND 输出新 token 后自动重连。
# bridge 的 Session Ingress transport 会按 reference 语义丢弃本地已发送 UUID 的 echo，
# 并忽略重复投递的 inbound user UUID，避免 WebSocket 重放导致同一用户消息重复执行。

# 构建特定 crate
cargo build -p kiana-tools
```

### 测试
```bash
# 运行所有测试
cargo test --workspace

# 测试电脑控制 MCP 的 native 后端（需要系统桌面开发库）
cargo test -p kiana-computer-mcp --features native
cargo test -p kiana-entrypoints --features native-computer-use cli::tests::computer_mcp_stdio_route_accepts_command_and_legacy_flags

# 运行特定 crate 的测试
cargo test -p kiana-types

# 带输出的测试
cargo test -- --nocapture
```

### 文档
```bash
# 生成文档
cargo doc --workspace --open

# 检查文档链接
cargo doc --workspace --no-deps
```

### Linting
```bash
# Clippy 检查
cargo clippy --workspace -- -D warnings

# 格式化代码
cargo fmt --all
```

## 🔧 配置

### 依赖版本管理
所有共享依赖在根 `Cargo.toml` 的 `[workspace.dependencies]` 中统一管理。

### 发布配置
- **LTO**: thin（链接时优化）
- **Codegen units**: 1（最优化）
- **Strip**: true（移除调试符号）
- **Opt-level**: 3（最高优化）

### 发布前 smoke
```bash
# 查看发布前 gate
./target/debug/kiana release

# 执行发布前 smoke：格式、workspace 测试、release build、版本、doctor 和临时安装检查
bash scripts/release-smoke.sh
```

CI 使用 `.github/workflows/release-smoke.yml` 调用同一个 `scripts/release-smoke.sh`，避免本地和 PR gate 分叉。

真实服务连通性可以用 `kiana remote-session code-session smoke --json` 单独验证，它会先创建 CCR v2 code session，再拉取 bridge credentials 和 SDK URL。这个 gate 需要 `KIANA_REMOTE_ACCESS_TOKEN` 或 `CLAUDE_ACCESS_TOKEN` 这类 remote bearer token；普通 `ANTHROPIC_API_KEY` / `sk-*` API key 不能用于 remote session 认证。

## 📚 相关文档

- [重构报告](REWRITE-REPORT.md) - 当前重构进度、已验证链路和未完成项
- [商业发布就绪清单](docs/commercial-release-readiness.md) - 本地 gate、打包产物和正式商用阻断项
- [发布核对清单](docs/release-checklist.md) - 从本地 RC 到商业发布的逐项 gate
- [升级指南](UPGRADE.md) - 版本升级、回滚和兼容性说明
- [更新日志](CHANGELOG.md) - 已发布版本、验证结果和已知阻塞项
- [分发渠道](docs/distribution-channels.md) - 当前可用渠道和商业发行目标
- [迁移路线图](docs/reference-migration-roadmap.md) - reference parity 迁移说明
- [参考审计](docs/reference_audit/) - 可提交的 reference 功能差距证据
- [安全策略](SECURITY.md) - 漏洞报告、依赖审计和 sandbox 边界
- [隐私说明](PRIVACY.md) - 本地数据、密钥、日志和远程传输
- [遥测说明](TELEMETRY.md) - 当前遥测状态和未来 opt-in 约束

## 🤝 贡献

欢迎贡献！请遵循：
1. Fork 项目
2. 创建 feature 分支
3. 提交 PR（包含测试）
4. 确保 `cargo clippy` 和 `cargo test` 通过

## 📄 License

MIT OR Apache-2.0

---

**从 TypeScript 到 Rust 的重构仍在推进**
*23 个 crate，reference parity 以当前测试和运行级 smoke 为准*
