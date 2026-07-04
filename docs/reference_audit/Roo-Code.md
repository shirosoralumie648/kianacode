# Roo-Code Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/Roo-Code` 对 Kiana 的参考重点是 IDE/product shell、mode/profile、tool validation、MCP hub、checkpoint/file change UI 和 terminal process 管理。它是 VS Code 扩展产品，Kiana 应借鉴用户可见行为、配置分层、工具校验和状态面板，不应把 VS Code webview 或扩展宿主假设搬进 Rust core。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| CLI entry | 用户可通过 Roo CLI 启动任务和测试入口 | `reference/Roo-Code/apps/cli/README.md`, `reference/Roo-Code/apps/cli/src/index.ts`, `reference/Roo-Code/apps/cli/src/__tests__/index.test.ts` | CLI 包独立于 VS Code core，测试校验参数入口 | `kiana-entrypoints/src/cli.rs` | 部分 | CLI 深度不如 Kiana Rust entrypoints |
| Tool validation | 工具调用缺参、非法参数、文件/命令执行被校验 | `reference/Roo-Code/src/core/tools/ReadFileTool.ts`, `reference/Roo-Code/src/core/tools/WriteToFileTool.ts`, `reference/Roo-Code/src/core/tools/ExecuteCommandTool.ts`, `reference/Roo-Code/src/core/tools/validateToolUse.ts`, `reference/Roo-Code/src/core/tools/__tests__/validateToolUse.spec.ts` | tool class 暴露 schema/execute，validateToolUse 在执行前返回结构化错误 | `kiana-tools/src/tool.rs`, `kiana-tools/src/registry.rs`, `kiana-tools/src/tool_execution.rs` | 是 | TS class hierarchy 不适合 Rust trait 直接复制 |
| Modes/profiles | 用户可切换 Code/Ask/Debug 等 mode 与 profile | `reference/Roo-Code/src/shared/modes.ts`, `reference/Roo-Code/src/shared/ProfileValidator.ts`, `reference/Roo-Code/apps/docs/docs/basic-usage/using-modes.md` | mode 定义 capability set，profile validator 限制不合法配置 | `kiana-types/src/plugin.rs`, `kiana-tools/src/permissions.rs`, future modes | 是 | 不能让 mode 绕过安全策略 |
| Product shell/webview | 用户在 IDE 面板中查看对话、工具、文件变化、设置 | `reference/Roo-Code/src/core/webview/ClineProvider.ts`, `reference/Roo-Code/src/core/webview/webviewMessageHandler.ts`, `reference/Roo-Code/webview-ui/src/App.tsx`, `reference/Roo-Code/webview-ui/src/__tests__/FileChangesPanel.spec.tsx` | core provider 通过 message handler 与 webview UI 通信 | `kiana-bridge/src`, `kiana-remote/src`, `kiana-screens/src` | 部分 | Webview state 不属于 Kiana core |
| MCP hub | 用户管理 MCP server 并调用 MCP tool | `reference/Roo-Code/src/services/mcp/McpHub.ts`, `reference/Roo-Code/src/services/mcp/McpServerManager.ts`, `reference/Roo-Code/src/services/mcp/__tests__/McpHub.spec.ts`, `reference/Roo-Code/apps/docs/docs/features/mcp/server-transports.md` | MCP hub 管理 server lifecycle，tool wrapper 转发调用 | `kiana-services/src/mcp.rs`, `kiana-tools/src/mcp_tool.rs` | 是 | 需按 Kiana transport/auth 模型重做 |
| Checkpoint/file changes | 用户可查看文件变化并恢复 checkpoint | `reference/Roo-Code/src/core/webview/checkpointRestoreHandler.ts`, `reference/Roo-Code/webview-ui/src/__tests__/fileChangesFromMessages.spec.ts`, `reference/Roo-Code/src/utils/git.ts` | 从消息提取 file changes，git utility 支撑恢复 | `kiana-commands/src/checkpoint.rs`, `kiana-commands/src/diff.rs` | 是 | UI 面板可后置，先补 CLI contract |
| Terminal process | 终端命令有 registry、process lifecycle 和 exec tests | `reference/Roo-Code/src/integrations/terminal/TerminalProcess.ts`, `reference/Roo-Code/src/integrations/terminal/TerminalRegistry.ts`, `reference/Roo-Code/src/integrations/terminal/__tests__/TerminalProcessExec.bash.spec.ts` | TerminalRegistry 管理多个 process，TerminalProcess 暴露输出和终止 | `kiana-tools/src/bash_tool.rs`, `kiana-tools/src/powershell_tool.rs` | 是 | PTY/terminal 交互需平台隔离 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Tool validation before execution

来源文件：
- `reference/Roo-Code/src/core/tools/validateToolUse.ts`
- `reference/Roo-Code/src/core/tools/__tests__/validateToolUse.spec.ts`
- `reference/Roo-Code/apps/docs/docs/advanced-usage/available-tools/tool-use-overview.md`

机制摘要：
- 工具执行前统一校验工具名、参数和 mode 权限。
- 缺失参数返回用户和模型都能理解的错误。
- tests 覆盖常见 invalid tool use。

Kiana 可借鉴方式：
- 在 `kiana-tools/src/tool_execution.rs` 把 schema validation 变成公共 gate。
- 让 stream-json/TUI/remote 看到同一种 ToolError。

不应该照搬的部分：
- 不复制 Roo 的 XML prompt tool syntax。
- 不让 UI docs 决定 core schema。

### Pattern: Mode as capability profile

来源文件：
- `reference/Roo-Code/src/shared/modes.ts`
- `reference/Roo-Code/src/shared/ProfileValidator.ts`
- `reference/Roo-Code/apps/docs/docs/basic-usage/using-modes.md`

机制摘要：
- mode 决定可用工具、提示词和交互期望。
- profile validator 防止非法组合。
- 用户可显式切换 mode。

Kiana 可借鉴方式：
- 把 Kiana 的 permission profile、tool allowlist、system prompt variant 合并成可解释的 mode contract。
- mode 不直接覆盖低层 sandbox。

不应该照搬的部分：
- 不复制 Roo 的 persona 文案。
- 不把 mode 写死在 UI 层。

## 4. Behavior Contracts to Port into Kiana

### Contract: Tool Input Validation

Input:
- tool name
- JSON input
- current mode/profile
- cwd and permission policy

Decision:
- tool 是否存在。
- required fields 是否存在且类型正确。
- mode/profile 是否允许该工具。

Execution:
- 在 handler 前运行 validation。
- validation failure 生成 ToolError。
- handler 不会被调用。

Output:
- typed validation error。
- model-facing repair hint。

Runtime Events:
- ToolCall
- ToolResult
- RuntimeError

Acceptance Tests:
- missing required argument 返回 ToolError。
- unknown tool 返回 unknown_tool error code。
- blocked-by-mode 不会执行 handler 或写文件。

### Contract: File Changes Panel Source Data

Input:
- session events
- tool results with file modifications
- checkpoint id

Decision:
- 哪些事件属于文件修改。
- diff 是否可从 git/worktree 生成。
- restore 是否需要 confirmation。

Execution:
- 从 RuntimeEvent 提取 file changes。
- 生成 stable diff summary。
- restore checkpoint 时发出 operation events。

Output:
- file change list。
- restore result。

Runtime Events:
- ToolResult
- SessionEvent
- RuntimeError

Acceptance Tests:
- Edit/Write/Delete 都出现在 file change list。
- 没有文件变化时返回空列表而不是错误。
- restore 失败时不会丢失当前 checkpoint。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Tool validation | registry/tool_execution 已有基础；validation failure 现在在 `tool_result.error` 输出稳定 `tool_validation_error`、原始 validation code、tool/use id 和 model-facing repair hint，并通过 RuntimeEvent、stream-json、remote/bridge adapter 保留同一个错误对象；TUI replay 现在也直接展示该 error metadata | 后续仍可继续增强可视样式，但结构化错误已不再只藏在底层事件中 | `kiana-tools/src/tool_execution.rs`, `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/tui.rs`, `kiana-remote/src/sdk_message_adapter.rs`, `kiana-bridge/src/sdk_message_adapter.rs` | `cargo test -p kiana-tools validation_failure_returns_structured_tool_error_with_repair_hint`; `cargo test -p kiana-types runtime_event_schema_covers_required_payloads`; `cargo test -p kiana-entrypoints maps_tool_result_error_metadata_to_conversation_messages`; `cargo test -p kiana-entrypoints stream_json_partial_event_exposes_tool_result_error_lifecycle_runtime_events`; `cargo test -p kiana-remote remote_sdk_adapter_emits_runtime_events_for_messages_tools_and_results`; `cargo test -p kiana-bridge bridge_sdk_adapter_emits_runtime_events_for_messages_tools_and_results` | P0 |
| Modes/profiles | permission profile 存在 | mode/profile 与 tool allowlist/system prompt 未形成用户概念 | `kiana-tools/src/permissions.rs`, `kiana-entrypoints/src/runner.rs` | mode/profile validation tests | P1 |
| File change data | diff/checkpoint 命令已有；`Write`/`Edit` 工具现在在 model-facing `tool_result.changed_files` 输出路径和操作类型，并通过 RuntimeEvent、stream-json、remote SDK adapter、bridge adapter、runtime/app-server schema 与 SDK 文档保留该元数据 | Delete 工具级 metadata、checkpoint restore 前后 file-change 对比、以及面向 UI 的聚合 file changes view 仍需继续补齐 | `kiana-tools/src/file_write.rs`, `kiana-tools/src/file_edit.rs`, `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/runner.rs`, `kiana-entrypoints/src/cli.rs`, `kiana-remote/src/sdk_message_adapter.rs`, `kiana-bridge/src/sdk_message_adapter.rs`, `kiana-commands/src/diff.rs`, `kiana-commands/src/checkpoint.rs` | `cargo test -p kiana-tools write_api_result_reports_changed_file_metadata`; `cargo test -p kiana-tools edit_api_result_reports_changed_file_metadata`; `cargo test -p kiana-entrypoints runner_runtime_event_adapter_preserves_changed_files`; `cargo test -p kiana-entrypoints stream_json_partial_event_exposes_tool_result_changed_files`; `cargo test -p kiana-remote remote_sdk_adapter_emits_runtime_events_for_messages_tools_and_results`; `cargo test -p kiana-bridge bridge_sdk_adapter_preserves_tool_result_changed_files`; `cargo test -p kiana-types runtime_event_schema_covers_required_payloads`; `bash scripts/schema-contract-smoke.sh` | P1 |
| Terminal process lifecycle | Bash/PowerShell 工具有实现 | long-running/PTY/kill/reconnect contract 仍需加强 | `kiana-tools/src/bash_tool.rs`, `kiana-tools/src/powershell_tool.rs` | timeout, kill, stderr ordering | P1 |

## 6. Atomic Implementation Tasks

### Task: Standardize Tool Validation Errors

Goal:
- 所有工具参数错误都通过统一 ToolError code 和 runtime event 输出。

Scope:
- 允许修改 `kiana-tools/src/tool_execution.rs`, `kiana-tools/src/tool.rs`, `kiana-tools/src/registry.rs`。
- 不允许改变工具业务逻辑。

Implementation Notes:
- 先定义 validation error enum。
- handler 前统一校验，handler 内只处理业务错误。

Acceptance Criteria:
- unknown tool、missing field、wrong type、blocked by profile 有不同 code。
- handler 未被调用时测试可证明无副作用。
- stream-json 和 TUI 使用同一错误内容。

Tests:
- `cargo test -p kiana-tools validation`
- `cargo test -p kiana-entrypoints stream_json_tool_errors`

Manual Verification:
- `kiana -p "call a non-existent tool" --output-format=stream-json`

### Task: Emit File Change Summaries From Runtime Events

Goal:
- 让 CLI/TUI/bridge 能从 session events 生成稳定 file changes view。

Scope:
- 允许修改 `kiana-tools/src/file_edit.rs`, `kiana-commands/src/diff.rs`, `kiana-commands/src/checkpoint.rs`, `kiana-types/src/runtime.rs`。
- 不允许把 UI state 写入 runtime events。

Implementation Notes:
- ToolResult 附带 changed_files metadata。
- diff command 读取 metadata，不重新解析 assistant 文本。

Acceptance Criteria:
- Edit/Write/Delete 工具均记录文件路径和操作类型。
- checkpoint restore 前后可比较 changed files。
- 没有 metadata 的 legacy session 仍可通过 git diff fallback。

Tests:
- `cargo test -p kiana-tools file_change_metadata`
- `cargo test -p kiana-entrypoints cli_diff`

Manual Verification:
- `kiana diff --session <id>`
