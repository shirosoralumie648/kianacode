# cline Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/cline` 对 Kiana 的价值在 SDK、IDE/product shell、provider gateway、hooks/plugins/skills 文档、CLI/headless/TUI 用法、checkpoint 和 eval/smoke。它是 TypeScript/VS Code 生态，Kiana 应借行为合同和测试组织，不应移植 VS Code-specific UI 或快速变化的 provider catalog。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| CLI / scheduling / headless | 用户可用 CLI 跑 agent、计划任务、连接器 | `reference/cline/docs/cli/cli-reference.mdx`, `reference/cline/docs/usage/cli-overview.mdx`, `reference/cline/apps/cli/src/index.ts`, `reference/cline/apps/cli/src/wizards/schedule/index.ts` | CLI 文档和 app CLI 负责 headless、wizards、scheduling | `kiana-entrypoints/src/cli.rs`, future automation | 部分 | Cline 云调度不适合 Kiana 当前核心 |
| SDK event and core API | SDK 文档定义 core、agent、tools、events、permission handling | `reference/cline/sdk/README.md`, `reference/cline/sdk/ARCHITECTURE.md`, `reference/cline/docs/sdk/reference/events.mdx`, `reference/cline/sdk/packages/shared/src/rpc/runtime.ts` | shared package 定义 runtime/RPC/event 类型，SDK 包提供稳定接口 | `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/sdk.rs` | 是 | SDK shape 需 Rust-first，不复制 TS API |
| Provider/model gateway | 用户配置 Anthropic/OpenAI-compatible/Bedrock/OpenRouter/local 等 | `reference/cline/docs/provider-config/anthropic.mdx`, `reference/cline/docs/provider-config/openai-compatible.mdx`, `reference/cline/apps/cline-hub/src/webview/src/lib/provider-model-catalog.ts`, `reference/cline/apps/vscode/src/utils/model-utils.ts` | provider config 和 model utils 统一管理 provider/model capabilities | `kiana-services/src/api`, `kiana-commands/src/model.rs` | 是 | Model lists 时间敏感，需 live-free registry tests |
| Tools and prompt registry | VS Code agent 工具、system prompt 变体、missing param tests | `reference/cline/apps/vscode/src/core/prompts/system-prompt/registry/PromptRegistry.ts`, `reference/cline/apps/vscode/src/core/prompts/system-prompt/tools/read_file.ts`, `reference/cline/apps/vscode/src/core/prompts/system-prompt/tools/execute_command.ts`, `reference/cline/apps/vscode/src/core/prompts/__tests__/toolSpecificMissingParamErrors.test.ts` | prompt registry 组装工具说明并按 provider/model 变体输出 | `kiana-tools/src/registry.rs`, `kiana-entrypoints/src/runner.rs` | 部分 | Prompt 内容和 provider-specific variants 不复制 |
| Hooks / plugins / skills | 文档展示用户自定义 hooks、skills、plugins | `reference/cline/docs/customization/hooks.mdx`, `reference/cline/docs/customization/skills.mdx`, `reference/cline/docs/customization/plugins.mdx`, `reference/cline/apps/vscode/src/core/hooks/__tests__/fixtures/hooks/pretooluse/blocking/PreToolUse` | hooks 有 fixture，plugin/skills 有 docs 和 UI | `kiana-skills/src`, `kiana-query/src/stop_hooks.rs` | 是 | UI config 不要移植 |
| Checkpoints / file workflow | 用户能 checkpoint、恢复、管理 task | `reference/cline/docs/core-workflows/checkpoints.mdx`, `reference/cline/docs/core-workflows/working-with-files.mdx`, `reference/cline/apps/vscode/src/utils/git-worktree.ts`, `reference/cline/apps/vscode/src/utils/worktree-include.ts` | worktree/git utility 管理 checkpoint 和 include paths | `kiana-commands/src/checkpoint.rs`, `kiana-tools/src/file_edit.rs` | 是 | VS Code workspace assumptions 需剥离 |
| Eval / smoke | SDK/CLI 发布前 smoke、eval 分析 | `reference/cline/sdk/scripts/ci-node-smoke.ts`, `reference/cline/evals/smoke-tests/README.md`, `reference/cline/evals/analysis/src/metrics.ts` | smoke scripts 和 eval metrics 分开 | `scripts/release-smoke.sh`, future `kiana-eval` | 是 | 不要让 default CI 依赖 paid APIs |

## 3. 值得概念性借鉴的实现模式

### Pattern: Stable SDK event contracts

来源文件：
- `reference/cline/docs/sdk/reference/events.mdx`
- `reference/cline/sdk/packages/shared/src/rpc/runtime.ts`
- `reference/cline/sdk/packages/shared/src/session/records.ts`

机制摘要：
- SDK 类型独立于 UI。
- session/runtime records 可被 CLI、IDE、hub 消费。
- 文档和 tests 锁定事件 shape。

Kiana 可借鉴方式：
- 把 `kiana-types/src/runtime.rs` 当 public SDK schema。
- 每次新增 event 同步 golden tests。

不应该照搬的部分：
- 不复制 TS enum/API names。
- 不绑定 Cline hub。

### Pattern: Provider config separated from UI

来源文件：
- `reference/cline/docs/provider-config/openai-compatible.mdx`
- `reference/cline/apps/cline-hub/src/webview/src/lib/provider-schema.ts`
- `reference/cline/apps/vscode/src/utils/model-utils.ts`

机制摘要：
- provider schema 描述 auth、base URL、model selection。
- UI 只消费 schema 和 model catalog。
- tests 覆盖 model utilities。

Kiana 可借鉴方式：
- 在 `kiana-services` 引入 Provider trait 和 ModelProfile registry。
- CLI `kiana model` 只读取 registry，不硬编码全部 provider UI。

不应该照搬的部分：
- 不复制 provider catalog 数据。
- 不把 Webview state 放进 core。

## 4. Behavior Contracts to Port into Kiana

### Contract: Provider Registry Selection

Input:
- provider id
- model id
- auth config/env
- optional base URL

Decision:
- provider 是否已注册。
- model 是否支持 tool/streaming/vision/structured output。
- auth 是否可解析。

Execution:
- 构建 provider client。
- fake provider 可无网络执行 tests。
- live smoke 必须 opt-in。

Output:
- selected provider/model profile。
- typed error if missing auth/capability。

Runtime Events:
- SessionEvent
- RuntimeError

Acceptance Tests:
- Anthropic default 不变。
- OpenAI-compatible fake provider 可完成 tool loop。
- Ollama/local provider auth optional 且不会请求云。

### Contract: SDK Event Subscription

Input:
- session id
- event stream subscription
- output format

Decision:
- 是否读取历史 replay。
- 是否只输出 public event fields。
- 是否处理 unknown future event。

Execution:
- 订阅 runner runtime events。
- 序列化成 public JSONL。
- consumer disconnect 时清理。

Output:
- ordered event stream。
- final result event。

Runtime Events:
- UserMessage
- AssistantMessage
- StreamDelta
- ToolCall
- ToolResult
- RuntimeResult

Acceptance Tests:
- tool error 以 event 输出。
- unknown event 不破坏 stream。
- reconnect/replay 不重复 terminal result。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Provider registry | Anthropic-first，model command 基础 | Provider trait、OpenAI-compatible、Ollama/mock profiles 缺失 | `kiana-services/src/api`, `kiana-services/src/auth.rs`, `kiana-commands/src/model.rs` | fake provider tool-loop tests | P0 |
| Public SDK docs | RuntimeEvent 已有 | SDK schema docs 和 compatibility policy 不完整 | `docs/`, `kiana-types/tests/runtime_event_schema.rs` | schema golden + docs links | P1 |
| Product shell | TUI/remote/bridge 有骨架 | local hub / IDE / webview shell 可后置 | `kiana-entrypoints/src/tui.rs`, `kiana-bridge/src` | protocol fixtures | P3 |
| Eval/smoke | release-smoke 已有 | Eval replay metrics 未有 | `scripts/`, future `kiana-eval` | deterministic replay | P2 |

## 6. Atomic Implementation Tasks

### Task: Provider Trait And Fake Provider

Goal:
- 引入最小 Provider trait，让 Kiana 可在无网络测试中跑完整 assistant/tool loop。

Scope:
- 允许修改 `kiana-services/src/api`, `kiana-entrypoints/src/runner.rs`, `kiana-commands/src/model.rs`。
- 不允许改变 Anthropic 默认行为。

Implementation Notes:
- 先实现 fake/mock provider 和 Anthropic adapter。
- ModelProfile 包含 tool_support、streaming、context_window。

Acceptance Criteria:
- fake provider 完成 Read/Edit/Bash tool loop。
- missing auth 有 typed error。
- Anthropic env path 仍通过现有 smoke。

Tests:
- `cargo test -p kiana-services provider`
- `cargo test -p kiana-entrypoints runner`

Manual Verification:
- `KIANA_PROVIDER=fake kiana -p "..." --tools Read`

### Task: SDK Runtime Event Documentation

Goal:
- 把 RuntimeEvent 写成 public SDK contract 文档并生成 fixtures。

Scope:
- 允许新增 `docs/sdk-runtime-events.md` 和扩展 `kiana-types/tests/runtime_event_schema.rs`。
- 不允许改 event 字段除非测试同步。

Implementation Notes:
- 每个 event 一个 JSON example。
- 明确 forward-compatible unknown event 策略。

Acceptance Criteria:
- docs 覆盖所有 current RuntimeEvent payload。
- tests 校验 examples 可反序列化。
- stream-json 输出字段与 docs 一致。

Tests:
- `cargo test -p kiana-types --test runtime_event_schema`
