# codex Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/codex` 是 Kiana 最重要的 Rust 结构参考，适合影响 CLI/print mode、runtime session、thread store、tool lifecycle、permission profile、MCP、plugin/skills、TUI 和 app-server/remote shell。Kiana 不应复制 Codex 的 OpenAI/ChatGPT 云服务耦合、实验性 desktop app-server 细节或品牌特定模型逻辑。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| CLI / print / doctor | `codex` 启动 TUI，子命令管理 MCP、插件、sandbox、doctor | `reference/codex/codex-rs/cli/src/main.rs`, `reference/codex/codex-rs/cli/src/mcp_cmd.rs`, `reference/codex/codex-rs/cli/src/plugin_cmd.rs`, `reference/codex/codex-rs/cli/src/doctor.rs` | clap CLI 分层，命令只进入相应处理器，doctor 聚合环境、git、runtime | `kiana-entrypoints/src/cli.rs`, `kiana-commands/src/doctor.rs` | 是 | CLI 行为可借鉴，OpenAI 登录和 desktop app 启动不要照搬 |
| Thread/session/event store | 用户可创建、读取、搜索、归档 thread，事件可映射到 app-server item | `reference/codex/codex-rs/thread-store/src/store.rs`, `reference/codex/codex-rs/thread-store/src/local/live_writer.rs`, `reference/codex/codex-rs/app-server-protocol/src/protocol/event_mapping.rs`, `reference/codex/codex-rs/core/src/session/turn.rs` | thread metadata 与 live writer 分离；turn 事件映射成协议 item | Kiana 已有 `kiana-types/src/runtime.rs` 和 JSONL session tree | 是 | app-server item schema 复杂，Kiana 先保留自己的 RuntimeEvent |
| Tool execution system | 工具有 schema、handler、runtime、parallel/read-only 和 lifecycle events | `reference/codex/codex-rs/core/src/tools/registry.rs`, `reference/codex/codex-rs/core/src/tools/router.rs`, `reference/codex/codex-rs/core/src/tools/parallel.rs`, `reference/codex/codex-rs/core/src/tools/handlers/shell.rs`, `reference/codex/codex-rs/core/src/tools/lifecycle.rs` | registry -> router -> handler/runtime；工具事件带 workbench/lifecycle 元数据 | `kiana-tools/src/registry.rs`, `kiana-tools/src/tool_execution.rs` | 是 | 不要复制 handler 结构；只迁移生命周期合同 |
| Permission / sandbox / exec policy | 用户按 profile 执行，危险命令请求批准或被拒绝 | `reference/codex/codex-rs/core/src/config/permission_profile_catalog.rs`, `reference/codex/codex-rs/core/src/config/resolved_permission_profile.rs`, `reference/codex/codex-rs/execpolicy/src/policy.rs`, `reference/codex/codex-rs/core/src/tools/sandboxing.rs`, `reference/codex/codex-rs/core/src/windows_sandbox.rs` | profile/catalog 解析成执行策略；execpolicy 独立解析命令规则；sandbox 层按平台汇总 | `kiana-tools/src/permissions.rs`, `kiana-tools/src/exec_policy.rs`, `kiana-tools/src/bash_sandbox.rs` | 是 | Windows sandbox 与 OpenAI managed policy 细节过重 |
| MCP / dynamic tools / resources | MCP 工具、resources、templates、prompts 被发现并作为工具暴露 | `reference/codex/codex-rs/rmcp-client/src/rmcp_client.rs`, `reference/codex/codex-rs/rmcp-client/src/stdio_server_launcher.rs`, `reference/codex/codex-rs/core/src/mcp.rs`, `reference/codex/codex-rs/core/src/tools/handlers/mcp_resource/read_mcp_resource.rs` | transport 客户端、认证状态、资源读取和工具暴露分层 | `kiana-services/src/mcp.rs`, `kiana-tools/src/mcp_tool.rs` | 是 | OAuth/elicitation 可后置；先保 stdio/http/sse/ws 合同 |
| TUI / app-server product shell | TUI 显示 history、tool call、permission、plugins；app-server 处理 RPC | `reference/codex/codex-rs/tui/src/app.rs`, `reference/codex/codex-rs/tui/src/history_cell/mod.rs`, `reference/codex/codex-rs/app-server/src/request_processors/thread_processor.rs`, `reference/codex/codex-rs/app-server-protocol/src/protocol/v2/thread.rs` | TUI 消费统一事件；app-server 以 request processor 隔离业务 | `kiana-entrypoints/src/tui.rs`, `kiana-screens/src`, `kiana-bridge/src` | 部分 | 目前 Kiana 应先补核心合同，避免过早移植完整 app-server |

## 3. 值得概念性借鉴的实现模式

### Pattern: Thread store with live event writer

来源文件：
- `reference/codex/codex-rs/thread-store/src/store.rs`
- `reference/codex/codex-rs/thread-store/src/local/live_writer.rs`
- `reference/codex/codex-rs/thread-store/src/local/read_thread.rs`

机制摘要：
- 会话元数据和事件追加写入分离。
- live writer 只追加事件，reader/search/archive 通过 store 接口操作。
- app-server 和 TUI 不直接依赖底层文件形态。

Kiana 可借鉴方式：
- 保留 `kiana-types/src/runtime.rs` 的 RuntimeEvent。
- 将 `kiana-entrypoints/src/sdk.rs` 的 JSONL 写入抽成稳定 session store API。
- 让 CLI/TUI/remote 都读取同一 store。

不应该照搬的部分：
- 不复制 Codex app-server item 类型。
- 不照搬 OpenAI thread metadata 字段。

### Pattern: Tool registry with permission and lifecycle gate

来源文件：
- `reference/codex/codex-rs/core/src/tools/registry.rs`
- `reference/codex/codex-rs/core/src/tools/router.rs`
- `reference/codex/codex-rs/core/src/tools/lifecycle.rs`
- `reference/codex/codex-rs/core/src/tools/handlers/request_permissions.rs`

机制摘要：
- registry 暴露工具 schema。
- router 根据 tool call 找 handler。
- 执行前检查权限、sandbox、network 等策略。
- 执行中发出 tool start/result/error 事件。

Kiana 可借鉴方式：
- 在 `kiana-tools/src/tool_execution.rs` 固化 ToolStart/ToolResult/ToolError runtime events。
- read-only 工具可批量并发，mutating 工具串行。
- permission request 使用统一 event，不让 UI/CLI 自己发明格式。

不应该照搬的部分：
- 不复制 Codex 的 handler module layout。
- 不把 tool UI 渲染逻辑混入 Kiana executor。

## 4. Behavior Contracts to Port into Kiana

### Contract: Thread Resume And Fork

Input:
- session/thread id
- cwd
- optional parent turn id

Decision:
- 判断 session 是否存在、是否属于当前 cwd 或允许跨 cwd。
- fork 时判断 parent events 是否可读。
- compact 后判断是否需要 rebuild event tree。

Execution:
- 从 session store 读取 metadata 和 events。
- resume 继续写入同一个 event tree。
- fork 创建新 session id，复制必要历史并记录 parent_session_id。

Output:
- 用户可见 session 摘要或 fork 后 id。
- 结构化 session metadata。

Runtime Events:
- SessionEvent
- UserMessage
- AssistantMessage
- ToolCall
- ToolResult

Acceptance Tests:
- resume 只继续目标 session。
- fork 后新 session 有 parent_session_id 且不再写回源 session。
- compact 后 resume 读取压缩后的 event tree。

### Contract: Tool Execution With Lifecycle Metadata

Input:
- tool name
- JSON input
- permission profile
- current session/turn id

Decision:
- 工具是否存在。
- 是否 read-only 可并发。
- 是否需要 permission request、sandbox 或 network approval。

Execution:
- 生成 ToolStart。
- 执行 handler/runtime。
- 捕获 stdout/stderr/content/error/duration。
- 将模型可读结果和内部结构化结果分离。

Output:
- model-facing tool_result。
- public runtime event。

Runtime Events:
- ToolCall
- PermissionRequest
- ToolResult
- RuntimeError

Acceptance Tests:
- unknown tool 返回 structured error。
- read-only batch 保持结果顺序。
- mutating tool 被权限拒绝时不会执行 handler。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Session store API | 已有 JSONL event tree 与 legacy JSON 兼容 | store API 仍散在 SDK/commands；thread metadata/search/index 不够独立 | `kiana-entrypoints/src/sdk.rs`, future `kiana-session` 或 `kiana-types` store module | session list/show/resume/fork/compact/import golden | P1 |
| Tool lifecycle | 已有 registry、tool_execution、runtime events；`tool_result.error` 现在会进入 runtime schema、stream-json、remote SDK adapter 和 bridge SDK adapter；`tool_result.changed_files` 现在覆盖 Write/Edit/Delete、RuntimeEvent、stream-json、remote/bridge adapter 和 schema；Delete 使用 read-before-mutate、mtime、editable/read-only guard | 未来新增 surfaces 的 lifecycle/error/file-change 断言仍需继续补齐；当前 local workflow 还需要最终 audit | `kiana-tools/src/tool_execution.rs`, `kiana-tools/src/file_write.rs`, `kiana-tools/src/file_edit.rs`, `kiana-tools/src/file_delete.rs`, `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/runner.rs`, `kiana-entrypoints/src/cli.rs`, `kiana-remote/src/sdk_message_adapter.rs`, `kiana-bridge/src/sdk_message_adapter.rs` | `cargo test -p kiana-tools delete_api_result_reports_changed_file_metadata`; `cargo test -p kiana-tools delete_requires_file_to_be_read_first`; `cargo test -p kiana-types runtime_event_schema_covers_required_payloads`; `cargo test -p kiana-entrypoints stream_json_partial_event_exposes_tool_result_error_lifecycle_runtime_events`; `cargo test -p kiana-entrypoints stream_json_partial_event_exposes_tool_result_changed_files`; `cargo test -p kiana-remote remote_sdk_adapter_emits_runtime_events_for_messages_tools_and_results`; `cargo test -p kiana-bridge bridge_sdk_adapter_preserves_tool_result_changed_files` | P0 |
| Permission profile | 已有 profile、managed policy、exec policy 基础 | sandbox readiness 和 platform policy 深度不足 | `kiana-tools/src/permissions.rs`, `kiana-tools/src/bash_sandbox.rs` | Bash/PowerShell/sandbox matrix | P0 |
| App-server protocol | Kiana 有 bridge/remote/direct-connect 基础 | 本地 app-server thread RPC 不是稳定 public contract | `kiana-bridge/src`, `kiana-remote/src`, future app server | JSON protocol fixtures | P2 |

## 6. Atomic Implementation Tasks

### Task: Normalize Session Store API

Goal:
- 给 Kiana 提供单一 session store 读写接口，隐藏 legacy JSON 与 JSONL event tree 细节。

Scope:
- 允许修改 `kiana-entrypoints/src/sdk.rs`、`kiana-commands/src/session.rs`。
- 不允许改变现有 session 文件兼容性。

Implementation Notes:
- 先抽小接口：read, append_event, rebuild_events, list_metadata。
- 保留现有 RuntimeEvent schema。

Acceptance Criteria:
- 所有 session 命令通过同一接口读写。
- legacy session 仍可 resume。
- JSONL-only session 仍可 export/import。

Tests:
- `cargo test -p kiana-entrypoints --test cli_session`
- `cargo test -p kiana-entrypoints --test cli_resume`

Manual Verification:
- `kiana session list`
- `kiana session fork <id>`

### Task: Complete Public Tool Lifecycle Fixtures

Goal:
- 让 CLI stream-json、TUI transcript、remote/bridge 都能看到一致的 ToolStart/ToolResult/ToolError。

Scope:
- 允许修改 `kiana-tools/src/tool_execution.rs`, `kiana-entrypoints/src/runner.rs`, `kiana-entrypoints/src/tui.rs`, `kiana-remote/src/sdk_message_adapter.rs`, `kiana-bridge/src/sdk_message_adapter.rs`。
- 不允许重写 tool trait。

Implementation Notes:
- 保留内部 `ToolOutput`。
- 增加缺失 surface 的 snapshots。

Acceptance Criteria:
- MCP isError、permission denied、timeout、validation error 都产生 runtime event。
- UI text 与 stream-json 不分叉。

Tests:
- `cargo test -p kiana-entrypoints runtime_event`
- `cargo test -p kiana-tools --lib`

Manual Verification:
- `kiana -p --input-format=stream-json --output-format=stream-json`
