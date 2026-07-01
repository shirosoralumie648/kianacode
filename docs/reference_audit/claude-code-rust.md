# claude-code-rust Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/claude-code-rust` 是一个 Rust 版 Claude Code 风格项目草图，覆盖 CLI/REPL、session/memory、tools、MCP、plugins/skills/hooks、GUI/web/remote 和 services。它与 Kiana 技术栈接近，但从审计角度应视为产品面枚举和模块边界参考，而不是成熟实现来源；Kiana 不应照搬其代码或未验证的服务实现。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| CLI/REPL | 用户通过 main/args/repl/commands/terminal 进入交互 | `reference/claude-code-rust/src/main.rs`, `reference/claude-code-rust/src/cli/args.rs`, `reference/claude-code-rust/src/cli/repl.rs`, `reference/claude-code-rust/src/cli/commands.rs`, `reference/claude-code-rust/src/terminal/mod.rs` | Rust CLI 和 REPL 模块分层 | `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/repl.rs` | 部分 | 实现成熟度需逐项验证 |
| Session/memory | 用户会话、历史、上下文和 consolidation 被保存 | `reference/claude-code-rust/src/session/mod.rs`, `reference/claude-code-rust/src/memory/session.rs`, `reference/claude-code-rust/src/memory/history.rs`, `reference/claude-code-rust/src/memory/context.rs`, `reference/claude-code-rust/src/memory/consolidation.rs` | session 与 memory 子模块分离，consolidation 处理压缩 | `kiana-commands/src/session.rs`, `kiana-commands/src/compact.rs` | 部分 | 不能替代 Kiana 已有 JSONL session tree |
| Core tools | 文件读写编辑、命令、git、task tools | `reference/claude-code-rust/src/tools/mod.rs`, `reference/claude-code-rust/src/tools/file_read.rs`, `reference/claude-code-rust/src/tools/file_write.rs`, `reference/claude-code-rust/src/tools/file_edit.rs`, `reference/claude-code-rust/src/tools/execute_command.rs`, `reference/claude-code-rust/src/tools/git_operations.rs`, `reference/claude-code-rust/src/tools/task_management.rs` | Rust tool modules 覆盖 coding agent 基础工具面 | `kiana-tools/src` | 部分 | 需看安全、权限、测试，不可直接信任 |
| MCP server | MCP tools/resources/prompts/transport server | `reference/claude-code-rust/src/mcp/server.rs`, `reference/claude-code-rust/src/mcp/tools.rs`, `reference/claude-code-rust/src/mcp/resources.rs`, `reference/claude-code-rust/src/mcp/prompts.rs`, `reference/claude-code-rust/src/mcp/transport.rs` | MCP server 分文件组织 tools/resources/prompts/transport | `kiana-services/src/mcp.rs`, `kiana-tools/src/mcp_tool.rs` | 部分 | Kiana 重点是 client/dynamic tools，不应转向 server-first |
| Plugins/skills/hooks | 用户可加载 plugin、hook、skill | `reference/claude-code-rust/src/plugins/loader.rs`, `reference/claude-code-rust/src/plugins/hooks.rs`, `reference/claude-code-rust/src/plugins/isolation.rs`, `reference/claude-code-rust/src/skills/registry.rs`, `reference/claude-code-rust/src/skills/executor.rs` | loader/hooks/isolation 和 skills registry/executor 分层 | `kiana-skills/src`, `kiana-commands/src/plugin.rs`, `kiana-commands/src/hooks.rs` | 部分 | isolation 是否足够需验证，不照搬 |
| GUI/web/remote | 用户可通过 GUI/web/wasm/remote/SSH 使用 | `reference/claude-code-rust/src/gui/app.rs`, `reference/claude-code-rust/src/gui/tool_calls.rs`, `reference/claude-code-rust/src/web/server.rs`, `reference/claude-code-rust/src/wasm/bridge.rs`, `reference/claude-code-rust/src/advanced/remote.rs`, `reference/claude-code-rust/src/advanced/ssh.rs` | UI/remote 模块枚举了多入口产品面 | `kiana-bridge/src`, `kiana-remote/src`, `kiana-screens/src` | 低 | 面太广，容易分散 P0 |
| Services/tests | marketplace、docs、agents 服务及 integration tests | `reference/claude-code-rust/src/services/plugin_marketplace.rs`, `reference/claude-code-rust/src/services/magic_docs.rs`, `reference/claude-code-rust/src/services/agents.rs`, `reference/claude-code-rust/tests/integration_test.rs`, `reference/claude-code-rust/tests/tools_test.rs`, `reference/claude-code-rust/tests/skills_test.rs` | 服务模块和测试展示预期能力 | `kiana-services/src`, future marketplace/docs | 低 | 服务可能是占位实现，需源码验证后再借 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Rust module surface map for Claude-Code-like product

来源文件：
- `reference/claude-code-rust/src/main.rs`
- `reference/claude-code-rust/src/tools/mod.rs`
- `reference/claude-code-rust/src/plugins/loader.rs`
- `reference/claude-code-rust/src/mcp/server.rs`

机制摘要：
- 用 Rust crate/module 方式罗列 CLI、tools、plugins、MCP、GUI/web。
- 对 Kiana 的差距盘点有目录层级参考价值。
- 比 TypeScript/Python reference 更接近 Rust ownership/trait 约束。

Kiana 可借鉴方式：
- 用它帮助检查 Kiana 是否遗漏了 product surface。
- 只把模块边界转成 Kiana task，不复制代码。

不应该照搬的部分：
- 不以该 repo 替代 Codex/Claude reverse repo 的成熟行为证据。
- 不复制未验证的 tool/security 实现。

### Pattern: Skills/plugins separated from core tools

来源文件：
- `reference/claude-code-rust/src/plugins/loader.rs`
- `reference/claude-code-rust/src/plugins/isolation.rs`
- `reference/claude-code-rust/src/skills/registry.rs`
- `reference/claude-code-rust/src/skills/executor.rs`

机制摘要：
- plugin loader 和 skill registry/executor 分开。
- isolation 是 plugin 层明确概念。
- core tools 不直接依赖 skill implementation。

Kiana 可借鉴方式：
- 保持 `kiana-skills/src` 与 `kiana-tools/src` 分层。
- plugin install/load/execute 必须产生 audit events。

不应该照搬的部分：
- 不采纳未经安全验证的 isolation 代码。
- 不把 marketplace 作为 P0。

## 4. Behavior Contracts to Port into Kiana

### Contract: Plugin/Skill Load Audit

Input:
- plugin directory
- skill manifest
- permission profile
- cwd

Decision:
- manifest 是否有效。
- plugin 是否受信任。
- skill 是否允许加载额外资源。

Execution:
- 解析 manifest。
- 加载 skill metadata。
- 发出 load audit event。

Output:
- registered skills/tools。
- load warning/error list。

Runtime Events:
- SessionEvent
- RuntimeError

Acceptance Tests:
- invalid manifest 不会注册 skill。
- untrusted plugin 在 restricted profile 下被拒绝。
- load warnings 可在 CLI/TUI 中查看。

### Contract: Rust Tool Module Parity Review

Input:
- Kiana tool registry
- expected coding tool list
- permission profile

Decision:
- 哪些基础工具缺失。
- 哪些工具已有但缺少 tests。
- 哪些工具应保持外部 plugin。

Execution:
- 枚举 registry。
- 比对 file/read/write/edit/bash/powershell/git/mcp/task 等能力。
- 输出 parity report。

Output:
- tool parity report。
- prioritized missing tests。

Runtime Events:
- SessionEvent

Acceptance Tests:
- parity report 包含所有 built-in tools。
- missing tool 有建议 crate/module。
- plugin-only capability 不被误判为 core P0。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Plugin/skill load audit | skills/plugins/hooks 已有基础 | load audit、trust、warning surfacing 需加强 | `kiana-skills/src`, `kiana-commands/src/plugin.rs`, `kiana-commands/src/skills.rs` | invalid manifest, untrusted plugin fixtures | P1 |
| Tool parity report | tools registry 存在 | 自动列出 built-in tools/capabilities/tests 的 doctor 命令缺失 | `kiana-tools/src/registry.rs`, `kiana-commands/src/doctor.rs` | registry snapshot | P2 |
| GUI/web/remote | bridge/remote/screen 有骨架 | 不应当前扩张到 GUI/web/SSH 全面产品面 | `kiana-bridge/src`, `kiana-remote/src` | protocol fixtures only | P3 |
| MCP server | Kiana MCP 重点在 client/tool | MCP server mode 不是当前必要能力 | `kiana-services/src/mcp.rs` | no immediate tests | P3 |

## 6. Atomic Implementation Tasks

### Task: Emit Plugin And Skill Load Audit Events

Goal:
- 用户能看到每个 plugin/skill 的加载结果、来源、警告和拒绝原因。

Scope:
- 允许修改 `kiana-skills/src`, `kiana-commands/src/plugin.rs`, `kiana-commands/src/skills.rs`, `kiana-types/src/runtime.rs`。
- 不允许改变 skill 文件格式，除非同步兼容迁移。

Implementation Notes:
- load audit event 应包含 plugin id、skill name、source path、status、warnings。
- restricted profile 下默认拒绝未信任 plugin。

Acceptance Criteria:
- invalid manifest 显示 typed error。
- untrusted plugin 不注册 executable skill。
- successful load 可被 `kiana skills list --json` 查看。

Tests:
- `cargo test -p kiana-skills`
- `cargo test -p kiana-entrypoints cli_skills`

Manual Verification:
- `kiana skills list --json`

### Task: Add Tool Registry Doctor Report

Goal:
- 让 `kiana doctor` 输出 built-in tools、MCP tools、plugin tools 的能力和测试覆盖提示。

Scope:
- 允许修改 `kiana-commands/src/doctor.rs`, `kiana-tools/src/registry.rs`。
- 不允许执行工具本身。

Implementation Notes:
- registry report 包含 tool name、read_only、requires_permission、source、schema status。
- 不显示 secrets。

Acceptance Criteria:
- doctor JSON 包含 built-in tool list。
- MCP/plugin tools 分组显示。
- schema invalid 的工具给出 warning。

Tests:
- `cargo test -p kiana-entrypoints doctor_tool_registry`
- `cargo test -p kiana-tools registry_snapshot`

Manual Verification:
- `kiana doctor --json`
