# claude-code-rev-main Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/claude-code-rev-main` 是 Claude Code 行为和产品细节的主要参考，适合影响 CLI/TUI 交互、Query/Task runtime、工具生命周期、权限、安全、MCP、plugins/skills/hooks、bridge/remote 和本地代码修改工作流。它是 reverse/recovered tree，Kiana 只能参考行为与边界，不能复制源码、私有 shims、隐藏服务或 UI 结构。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| CLI / REPL / input routing | 用户输入可被识别为文本、slash command、bash mode 或 prompt flow | `reference/claude-code-rev-main/src/main.tsx`, `reference/claude-code-rev-main/src/utils/processUserInput/processUserInput.ts`, `reference/claude-code-rev-main/src/utils/processUserInput/processSlashCommand.tsx`, `reference/claude-code-rev-main/src/utils/streamJsonStdoutGuard.ts` | 输入先分类，再路由到 command、bash、assistant runner 或 stream-json 输出保护 | `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/repl.rs` | 是 | React/Ink UI 和 recovered code 不能照搬 |
| Tool orchestration | 工具调用显示进度、权限、结果、错误，支持 streaming executor | `reference/claude-code-rev-main/src/services/tools/toolExecution.ts`, `reference/claude-code-rev-main/src/services/tools/StreamingToolExecutor.ts`, `reference/claude-code-rev-main/src/services/tools/toolOrchestration.ts`, `reference/claude-code-rev-main/src/utils/groupToolUses.ts` | 执行层与 UI/summary 分离；工具可分组、流式更新、错误汇总 | `kiana-tools/src/tool_execution.rs`, `kiana-entrypoints/src/runner.rs` | 是 | 不要复制 TS executor；只迁移事件顺序和错误合同 |
| Bash/PowerShell safety | 执行命令前识别危险模式、read-only 限制、sandbox 选择 | `reference/claude-code-rev-main/src/tools/BashTool/BashTool.tsx`, `reference/claude-code-rev-main/src/tools/BashTool/bashPermissions.ts`, `reference/claude-code-rev-main/src/tools/BashTool/destructiveCommandWarning.ts`, `reference/claude-code-rev-main/src/tools/PowerShellTool/powershellSecurity.ts` | command semantics、危险模式、路径规则、permission mode 共同决定是否允许 | `kiana-tools/src/bash_tool.rs`, `kiana-tools/src/powershell_tool.rs`, `kiana-tools/src/exec_policy.rs` | 是 | yolo/classifier prompt 不应直接复制 |
| MCP | MCP server 配置、auth、channel allowlist、tool/resource 调用 | `reference/claude-code-rev-main/src/services/mcp/client.ts`, `reference/claude-code-rev-main/src/services/mcp/MCPConnectionManager.tsx`, `reference/claude-code-rev-main/src/tools/MCPTool/MCPTool.ts`, `reference/claude-code-rev-main/src/tools/ReadMcpResourceTool/ReadMcpResourceTool.ts` | client manager 维护连接，工具层调用 MCP，UI 展示状态和审批 | `kiana-services/src/mcp.rs`, `kiana-tools/src/mcp_tool.rs` | 是 | UI/Claude account auth 细节有 license 与服务耦合风险 |
| Plugins / skills / hooks | 插件可提供 commands、agents、skills、hooks、MCP/LSP 集成 | `reference/claude-code-rev-main/src/utils/plugins/pluginLoader.ts`, `reference/claude-code-rev-main/src/utils/plugins/loadPluginCommands.ts`, `reference/claude-code-rev-main/src/utils/plugins/loadPluginHooks.ts`, `reference/claude-code-rev-main/src/utils/hooks/hookEvents.ts`, `reference/claude-code-rev-main/src/tools/SkillTool/SkillTool.ts` | plugin loader 聚合资源；hook registry 在 lifecycle 事件上运行 | `kiana-skills/src`, `kiana-commands/src/plugin.rs`, `kiana-query/src/stop_hooks.rs` | 是 | Kiana 已有实现，需补 parity fixtures，不能搬 schema code |
| Remote bridge / CCR | 本地 agent 连接远端 work，处理 session ingress、permission callbacks、token refresh | `reference/claude-code-rev-main/src/bridge/bridgeMain.ts`, `reference/claude-code-rev-main/src/bridge/codeSessionApi.ts`, `reference/claude-code-rev-main/src/bridge/bridgePermissionCallbacks.ts`, `reference/claude-code-rev-main/src/services/api/sessionIngress.ts` | bridge loop 轮询/连接，远端 user event 进入本地 runner，结果回传 | `kiana-bridge/src`, `kiana-remote/src` | 是 | 真实服务协议和 token flow 不应无测试迁移 |
| Settings / managed policy | 用户 settings、managed settings、permission validation、MDM raw read | `reference/claude-code-rev-main/src/utils/settings/settings.ts`, `reference/claude-code-rev-main/src/utils/settings/permissionValidation.ts`, `reference/claude-code-rev-main/src/utils/settings/mdm/settings.ts`, `reference/claude-code-rev-main/src/utils/permissions/permissionsLoader.ts` | 多来源配置叠加，managed policy 覆盖用户层，错误可报告 | `kiana-bootstrap/src/config.rs`, `kiana-commands/src/permissions.rs` | 是 | MDM/enterprise 细节可后置 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Input classification before assistant execution

来源文件：
- `reference/claude-code-rev-main/src/utils/processUserInput/processUserInput.ts`
- `reference/claude-code-rev-main/src/utils/processUserInput/processSlashCommand.tsx`
- `reference/claude-code-rev-main/src/utils/processUserInput/processBashCommand.tsx`

机制摘要：
- 用户输入先被分类为 slash command、bash command、text prompt 或特殊模式。
- 本地命令不进入模型。
- prompt 类输入才进入 assistant runner。

Kiana 可借鉴方式：
- 继续让 `kiana-commands` 成为 REPL/TUI/CLI 同源命令层。
- 给每种输入分类产生 RuntimeEvent 或 local command result。

不应该照搬的部分：
- 不复制 Ink component 逻辑。
- 不把 UI 状态混入 parser。

### Pattern: Hook outcomes as policy input

来源文件：
- `reference/claude-code-rev-main/src/utils/hooks/hookEvents.ts`
- `reference/claude-code-rev-main/src/utils/hooks/AsyncHookRegistry.ts`
- `reference/claude-code-rev-main/src/services/tools/toolHooks.ts`

机制摘要：
- hooks 按 lifecycle 事件注册。
- PreToolUse 可阻断、询问、修改输入。
- SessionStart/UserPromptSubmit 可追加上下文。

Kiana 可借鉴方式：
- 保留 `kiana-query/src/stop_hooks.rs` 的 hook runner。
- 扩展 tests 覆盖 project trust、plugin visibility、deny/ask/update_input/add_context。

不应该照搬的部分：
- 不复制 HTTP/agent hook executor。
- 不让 hook 直接绕过 permission layer。

## 4. Behavior Contracts to Port into Kiana

### Contract: User Input Routing

Input:
- raw user text
- current mode
- cwd/session state

Decision:
- 是否 slash command。
- 是否 shell shortcut。
- 是否需要本地处理或进入 model runner。

Execution:
- 解析本地命令并执行。
- prompt 类输入进入 assistant turn。
- 错误以用户可读文本和 RuntimeError 表示。

Output:
- command result 或 assistant stream。
- stream-json 模式输出稳定 JSONL。

Runtime Events:
- SessionEvent
- UserMessage
- RuntimeError
- RuntimeResult

Acceptance Tests:
- `/help` 不会进入模型。
- unknown slash command 返回本地错误。
- `-p` print mode 只输出 assistant result，stream-json 输出可解析。

### Contract: PreToolUse Permission Gate

Input:
- tool name
- tool input
- session/app state
- configured hooks

Decision:
- project 是否 trusted。
- hook 是否 block/deny/ask/update_input。
- permission profile 是否允许。

Execution:
- 执行 trusted hook sources。
- 应用 update_input。
- ask 走 permission prompt。
- 通过后才调用 tool。

Output:
- tool result 或 denied error。
- hook reason 暴露给用户。

Runtime Events:
- ToolCall
- PermissionRequest
- ToolResult
- RuntimeError

Acceptance Tests:
- block hook 阻止文件写入。
- update_input 在 validation 前生效。
- ask hook 被拒绝时 tool 不执行。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Input routing | CLI/REPL/TUI 已接本地 command registry | bash shortcut、mode transition、stream-json guard 仍需更完整 | `kiana-entrypoints/src/repl.rs`, `kiana-entrypoints/src/tui.rs`, `kiana-commands/src/registry.rs` | REPL/TUI command routing tests | P1 |
| Hooks | PreToolUse/PostToolUse/SessionStart/UserPromptSubmit 已有基础 | plugin enable/disable visibility 和 project trust fixtures 未完全收口 | `kiana-query/src/stop_hooks.rs`, `kiana-tools/src/tool_execution.rs` | hook source matrix tests | P1 |
| Remote bridge | mock CCR v2 和 Session Ingress 覆盖较多 | 真实服务 live smoke、token/epoch refresh 仍未完成 | `kiana-bridge/src`, `kiana-remote/src` | opt-in live smoke + mock protocol fixtures | P1 |
| Settings policy | 有 config overlay 与 managed policy | enterprise/MDM 等高级路径未覆盖 | `kiana-bootstrap/src/config.rs`, `kiana-commands/src/config.rs` | policy precedence fixtures | P2 |

## 6. Atomic Implementation Tasks

### Task: Add REPL Input Classification Contract Tests

Goal:
- 用测试锁定 slash/text/bash/unknown command 的路由，不改核心 runner。

Scope:
- 允许修改 `kiana-entrypoints/src/repl.rs`, `kiana-entrypoints/src/tui.rs`, `kiana-commands/src/registry.rs` 测试。
- 不允许改变 tool execution。

Implementation Notes:
- 使用 fake runner 或 record-only path。
- 输出保持现有格式。

Acceptance Criteria:
- 本地命令不进入模型。
- prompt 命令展开后进入模型。
- unknown 命令有明确错误。

Tests:
- `cargo test -p kiana-entrypoints repl`
- `cargo test -p kiana-entrypoints --lib tui::tests::`

Manual Verification:
- `kiana tui` 中运行 `/help`, `/doctor`, `/unknown`。

### Task: Complete Hook Source Visibility Matrix

Goal:
- 覆盖 env/home/project/plugin/frontmatter hooks 在 trusted/untrusted 和 plugin disabled 情况下的可见性。

Scope:
- 允许修改 `kiana-query/src/stop_hooks.rs`, `kiana-skills/src`, `kiana-commands/src/plugin.rs` 的测试和小修。
- 不允许新增 hook executor 类型。

Implementation Notes:
- 复用现有 hook outcome contract。
- 每个 source 单独 fixture。

Acceptance Criteria:
- untrusted project 不加载 project hooks。
- disabled plugin 不加载 plugin hooks。
- env/home hooks 仍可用。

Tests:
- `cargo test -p kiana-query --lib`
- `cargo test -p kiana-tools pre_tool_use_hook`
