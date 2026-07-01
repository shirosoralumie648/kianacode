# pi Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/pi` 适合作为 Kiana 的轻量 agent runtime、JSONL RPC、project trust、provider registry、TUI component 和 extension/skills 执行参考。它的价值不在 UI 视觉或 TypeScript API，而在小型 agent session 如何把 print mode、interactive mode、RPC mode、trust gate 和可测试事件流连接起来。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| CLI / print mode | 用户可通过 CLI 参数进入 print、interactive、RPC 等模式 | `reference/pi/packages/coding-agent/src/cli.ts`, `reference/pi/packages/coding-agent/src/cli/args.ts`, `reference/pi/packages/coding-agent/test/print-mode.test.ts`, `reference/pi/packages/coding-agent/docs/usage.md` | CLI 参数解析后选择 mode runner，print mode 有独立测试 | `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/runner.rs` | 是 | TypeScript commander 风格不需要照搬 |
| Session runtime events | 用户任务在 session runtime 中产生可断言事件 | `reference/pi/packages/coding-agent/src/core/agent-session.ts`, `reference/pi/packages/coding-agent/src/core/agent-session-runtime.ts`, `reference/pi/packages/coding-agent/src/core/session-manager.ts`, `reference/pi/packages/coding-agent/test/agent-session-runtime-events.test.ts` | agent session 管理状态，runtime 负责执行和事件广播，manager 负责生命周期 | `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/sdk.rs` | 是 | 事件名应按 Kiana RuntimeEvent 固化 |
| JSONL RPC | 外部程序可通过 JSONL 请求/响应驱动 agent | `reference/pi/packages/coding-agent/src/modes/rpc/rpc-mode.ts`, `reference/pi/packages/coding-agent/src/modes/rpc/jsonl.ts`, `reference/pi/packages/coding-agent/test/rpc-jsonl.test.ts`, `reference/pi/packages/coding-agent/test/rpc.test.ts` | JSONL parser 和 RPC mode 分离，测试覆盖 invalid input 与事件输出 | `kiana-entrypoints/src/runner.rs`, `kiana-bridge/src`, `kiana-remote/src` | 是 | 协议字段需 Rust-first，避免兼容 TS 内部对象 |
| Provider registry | 用户可选择 Anthropic/OpenAI/OpenRouter/faux provider | `reference/pi/packages/ai/src/api-registry.ts`, `reference/pi/packages/ai/src/models.ts`, `reference/pi/packages/ai/src/providers/all.ts`, `reference/pi/packages/ai/src/providers/faux.ts` | provider registry 汇总 provider、model 和 faux 测试后端 | `kiana-services/src/api`, `kiana-commands/src/model.rs` | 是 | provider 列表时间敏感，只借 registry 和 faux pattern |
| Project trust | 首次进入项目时要求用户信任或拒绝 | `reference/pi/packages/coding-agent/src/core/project-trust.ts`, `reference/pi/packages/coding-agent/src/cli/project-trust.ts`, `reference/pi/packages/coding-agent/test/trust-manager.test.ts`, `reference/pi/packages/coding-agent/test/trust-selector.test.ts` | trust manager 记录项目状态，CLI selector 决策是否继续 | `kiana-tools/src/permissions.rs`, `kiana-commands/src/trust.rs` | 是 | 不应把 trust 与 provider 登录耦合 |
| TUI components | interactive mode 显示 input、select list、tool execution | `reference/pi/packages/tui/src/tui.ts`, `reference/pi/packages/tui/src/components/input.ts`, `reference/pi/packages/tui/src/components/select-list.ts`, `reference/pi/packages/coding-agent/src/modes/interactive/interactive-mode.ts`, `reference/pi/packages/coding-agent/src/modes/interactive/components/tool-execution.ts` | TUI component 消费 runtime state，不拥有核心执行逻辑 | `kiana-entrypoints/src/tui.rs`, `kiana-screens/src` | 部分 | TS terminal rendering 不适合直接移植 |
| Extensions and skills | 用户可加载扩展并运行 skill | `reference/pi/packages/coding-agent/src/core/extensions/loader.ts`, `reference/pi/packages/coding-agent/src/core/extensions/runner.ts`, `reference/pi/packages/coding-agent/src/core/skills.ts`, `reference/pi/packages/coding-agent/examples/extensions/permission-gate.ts`, `reference/pi/packages/coding-agent/test/sdk-skills.test.ts` | extension loader 找到扩展，runner 提供事件和权限回调 | `kiana-skills/src`, `kiana-commands/src/plugin.rs`, `kiana-query/src/stop_hooks.rs` | 是 | 扩展隔离和权限必须强于示例实现 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Mode runner over shared session runtime

来源文件：
- `reference/pi/packages/coding-agent/src/core/agent-session-runtime.ts`
- `reference/pi/packages/coding-agent/src/modes/rpc/rpc-mode.ts`
- `reference/pi/packages/coding-agent/src/modes/interactive/interactive-mode.ts`

机制摘要：
- print、interactive、RPC 模式共用 session runtime。
- mode 只负责输入输出、订阅事件和终止条件。
- runtime event tests 锁定行为。

Kiana 可借鉴方式：
- 让 `kiana-entrypoints/src/runner.rs` 成为 print/TUI/bridge 的唯一执行入口。
- CLI/TUI/RPC 只转换输入输出，不重建 tool loop。

不应该照搬的部分：
- 不复制 TypeScript session object shape。
- 不把 TUI state 写入核心 runtime。

### Pattern: Faux provider for deterministic runtime tests

来源文件：
- `reference/pi/packages/ai/src/providers/faux.ts`
- `reference/pi/packages/ai/src/api-registry.ts`
- `reference/pi/packages/coding-agent/test/agent-session-runtime-events.test.ts`

机制摘要：
- faux provider 模拟模型响应、tool call 和终止。
- runtime tests 不依赖真实 API。
- provider registry 把测试 provider 当普通 provider 处理。

Kiana 可借鉴方式：
- 在 `kiana-services/src/api` 增加 fake provider。
- 用 fake provider 覆盖 tool-loop、permission、stream-json、resume/fork。

不应该照搬的部分：
- 不使用 faux 的具体 response DSL。
- 不在 release binary 默认暴露测试 provider，除非明确 opt-in。

## 4. Behavior Contracts to Port into Kiana

### Contract: JSONL RPC Mode

Input:
- stdin JSONL request
- session id or new session request
- cwd and permission profile

Decision:
- request 是否符合 protocol schema。
- 是否创建、resume 或关闭 session。
- 事件是否需要 replay。

Execution:
- 每行解析成 typed request。
- 调用 shared runner。
- 将 runtime events 写成 JSONL response。

Output:
- ordered JSONL events。
- terminal result or typed error。

Runtime Events:
- SessionEvent
- UserMessage
- AssistantMessage
- ToolCall
- ToolResult
- RuntimeError

Acceptance Tests:
- invalid JSONL 返回 typed error 且进程不 panic。
- 同一个 request id 的事件保持顺序。
- session close 后继续发送消息返回 protocol error。

### Contract: Project Trust Gate

Input:
- cwd
- trust policy
- interactive or non-interactive mode

Decision:
- cwd 是否已信任。
- 当前命令是否允许在 untrusted project 中运行。
- 非交互模式是否可自动拒绝。

Execution:
- 在 tool execution 前检查 trust。
- 需要确认时发出 permission/trust request。
- 记录用户选择。

Output:
- trust accepted/denied status。
- blocked runtime error for unsafe execution。

Runtime Events:
- PermissionRequest
- SessionEvent
- RuntimeError

Acceptance Tests:
- untrusted cwd 中 mutating tool 不会执行。
- non-interactive 默认拒绝并返回清晰错误。
- trusted cwd 后续运行不重复提示。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Shared mode runner | print/TUI/remote 已有基础 | 仍需确认所有 mode 共享同一 runner event contract | `kiana-entrypoints/src/runner.rs`, `kiana-entrypoints/src/tui.rs`, `kiana-bridge/src` | print/TUI/bridge golden events | P0 |
| JSONL RPC | stream-json 和 bridge 基础存在 | request/response protocol 的 public schema 与 replay 规则不足 | `kiana-entrypoints/src/runner.rs`, `kiana-bridge/src` | invalid JSON, resume, close fixtures | P1 |
| Fake provider | 尚未形成完整 provider registry | 无网络 tool-loop 测试能力不足 | `kiana-services/src/api`, `kiana-entrypoints/src/runner.rs` | fake provider runtime tests | P0 |
| Project trust | permissions/exec policy 有基础 | project trust UX 与 untrusted cwd gate 需要补齐 | `kiana-tools/src/permissions.rs`, `kiana-commands/src/trust.rs` | trust selector and non-interactive tests | P0 |

## 6. Atomic Implementation Tasks

### Task: Add Deterministic Fake Provider

Goal:
- 让 Kiana 在无网络环境中测试完整模型响应、tool call、tool result 和 final answer。

Scope:
- 允许修改 `kiana-services/src/api`, `kiana-entrypoints/src/runner.rs`, `kiana-commands/src/model.rs`。
- 不允许改变 Anthropic 默认 provider 行为。

Implementation Notes:
- fake provider 只在 env/config 显式选择时启用。
- 支持脚本化输出：assistant text、tool call、final answer。

Acceptance Criteria:
- fake provider 可驱动一个 Read 工具调用。
- fake provider 可驱动 permission denied 分支。
- stream-json 输出与 RuntimeEvent schema 一致。

Tests:
- `cargo test -p kiana-services provider`
- `cargo test -p kiana-entrypoints runner`

Manual Verification:
- `KIANA_PROVIDER=fake kiana -p "read README and summarize"`

### Task: Harden Project Trust Gate

Goal:
- 在未信任项目中默认阻断 mutating tools，并给交互模式提供明确确认流程。

Scope:
- 允许修改 `kiana-tools/src/permissions.rs`, `kiana-entrypoints/src/cli.rs`, `kiana-commands/src/trust.rs`。
- 不允许绕过现有 exec policy。

Implementation Notes:
- trust check 应在 tool handler 执行前。
- non-interactive mode 默认 fail closed。

Acceptance Criteria:
- untrusted cwd 中 Bash/Edit/Write 不执行。
- Read/list 类工具可按 policy 明确允许。
- trust 状态可 list、reset、explain。

Tests:
- `cargo test -p kiana-tools trust`
- `cargo test -p kiana-entrypoints cli_trust`

Manual Verification:
- `kiana trust status`
- `kiana -p "edit a file" --permission-profile default`
