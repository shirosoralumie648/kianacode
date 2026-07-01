# OpenHands Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/OpenHands` 适合作为 Kiana 未来 server/app conversation、sandbox service、browser/IDE product shell、settings/secrets、MCP/skills routes 和 evaluation harness 的高层参考。它是 Python/React 的完整应用平台，Kiana 当前不应照搬容器平台或前端架构，而应抽取 server conversation、sandbox boundary 和 event-to-UI contract。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| App conversation server | 用户通过 server 管理 conversation、事件和 websocket | `reference/OpenHands/openhands/app_server/README.md`, `reference/OpenHands/openhands/app_server/app_conversation/README.md`, `reference/OpenHands/openhands/app_server/event/README.md`, `reference/OpenHands/tests/unit/app_server/test_app_conversation_router.py` | app_server 按 conversation 组织事件、路由和连接 | `kiana-bridge/src`, `kiana-remote/src`, future app server | 部分 | 平台规模大，Kiana 先只借协议边界 |
| Sandbox service | 用户任务在 Docker/runtime sandbox 中执行 | `reference/OpenHands/openhands/app_server/sandbox/README.md`, `reference/OpenHands/tests/unit/app_server/test_docker_sandbox_service.py`, `reference/OpenHands/containers/README.md` | sandbox service 管理容器、文件和命令执行隔离 | `kiana-tools/src/bash_sandbox.rs`, `kiana-tools/src/permissions.rs` | 部分 | Docker runtime 过重，Windows/本地优先需另设策略 |
| Frontend conversation shell | 用户在网页中看对话、skills、hooks、files | `reference/OpenHands/frontend/src/components/features/conversation-panel/conversation-panel.tsx`, `reference/OpenHands/frontend/src/components/features/conversation-panel/skills-modal.tsx`, `reference/OpenHands/frontend/src/components/features/conversation-panel/hooks-modal.tsx`, `reference/OpenHands/frontend/src/components/features/files/file-list.tsx` | React UI 订阅 event store，conversation panel 与 files panel 分离 | `kiana-screens/src`, `kiana-bridge/src` | 部分 | UI 不迁移，只借事件数据需求 |
| Event UI handling | websocket/event store 把 backend events 转成 UI state | `reference/OpenHands/frontend/__tests__/utils/handle-event-for-ui.test.ts`, `reference/OpenHands/frontend/__tests__/stores/use-event-store.test.ts`, `reference/OpenHands/frontend/__tests__/conversation-websocket-handler.test.tsx` | 前端 tests 锁定 event-to-ui reducer 和 websocket handler | `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/tui.rs`, `kiana-bridge/src` | 是 | 不使用 OpenHands event schema 取代 Kiana RuntimeEvent |
| MCP/skills routes | 用户可通过 server API 管理 MCP 与 skills | `reference/OpenHands/tests/unit/mcp/test_mcp_integration.py`, `reference/OpenHands/tests/unit/server/routes/test_mcp_routes.py`, `reference/OpenHands/tests/unit/server/routes/test_skills_api.py`, `reference/OpenHands/skills/README.md` | server routes 暴露 MCP/skills 管理，tests 覆盖 API | `kiana-services/src/mcp.rs`, `kiana-skills/src`, `kiana-commands/src/skills.rs` | 是 | Route shape 可后置，先保 CLI contract |
| Settings/secrets/git | 用户可配置 settings、secrets、git settings | `reference/OpenHands/frontend/__tests__/routes/settings.test.tsx`, `reference/OpenHands/frontend/__tests__/routes/secrets-settings.test.tsx`, `reference/OpenHands/frontend/__tests__/routes/git-settings.test.tsx`, `reference/OpenHands/openhands/app_server/app_conversation/git/README.md` | settings routes/UI 与 git conversation 管理分离 | `kiana-services/src/auth.rs`, `kiana-commands/src/config.rs`, `kiana-commands/src/checkpoint.rs` | 部分 | secrets 存储必须按 Kiana 本地安全模型重做 |
| API/eval quality gates | OpenAPI schema、conversation router 有单元测试 | `reference/OpenHands/tests/unit/server/test_openapi_schema_generation.py`, `reference/OpenHands/tests/unit/app_server/test_app_conversation_router.py` | server API schema 可生成并测试，conversation router 有 fixtures | `kiana-bridge/src`, future app server | 是 | 不引入 Python server 依赖 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Conversation event reducer tests

来源文件：
- `reference/OpenHands/frontend/__tests__/utils/handle-event-for-ui.test.ts`
- `reference/OpenHands/frontend/__tests__/stores/use-event-store.test.ts`
- `reference/OpenHands/frontend/__tests__/conversation-websocket-handler.test.tsx`

机制摘要：
- UI 不直接理解 agent 内部状态。
- backend event 经 reducer 转成 conversation view state。
- websocket handler 和 store 均有 tests。

Kiana 可借鉴方式：
- 为 `kiana-types/src/runtime.rs` 增加 UI reducer fixtures。
- TUI/bridge/remote 都以 RuntimeEvent 为唯一输入。

不应该照搬的部分：
- 不复制 React store。
- 不替换 Kiana event schema。

### Pattern: Sandbox service boundary

来源文件：
- `reference/OpenHands/openhands/app_server/sandbox/README.md`
- `reference/OpenHands/tests/unit/app_server/test_docker_sandbox_service.py`
- `reference/OpenHands/containers/README.md`

机制摘要：
- server 不直接执行命令，而是通过 sandbox service。
- Docker sandbox 有生命周期 tests。
- 容器镜像和执行 API 分开。

Kiana 可借鉴方式：
- 把 `kiana-tools/src/bash_sandbox.rs` 作为 sandbox provider boundary。
- 本地 shell、Windows policy、未来 container sandbox 共用同一 trait/contract。

不应该照搬的部分：
- 不默认要求 Docker。
- 不把容器生命周期耦合进 CLI print mode。

## 4. Behavior Contracts to Port into Kiana

### Contract: App Conversation Event API

Input:
- conversation/session id
- optional replay cursor
- new user message or control command

Decision:
- session 是否存在。
- 是否需要 replay events。
- command 是否允许修改 session。

Execution:
- 读取 session store。
- 订阅 runner RuntimeEvent。
- 通过 websocket/JSONL 输出 event。

Output:
- ordered conversation events。
- terminal result and close reason。

Runtime Events:
- SessionEvent
- UserMessage
- AssistantMessage
- ToolCall
- ToolResult
- RuntimeResult

Acceptance Tests:
- replay cursor 之后只返回新增事件。
- websocket disconnect 不破坏 session 写入。
- unknown session 返回 typed not_found error。

### Contract: Sandbox Provider Boundary

Input:
- command
- cwd
- permission profile
- sandbox provider selection

Decision:
- provider 是否可用。
- 命令是否需要隔离。
- sandbox 启动失败是否 fallback。

Execution:
- 构建 sandbox execution request。
- 执行命令并收集 stdout/stderr/exit code。
- 清理或复用 sandbox。

Output:
- execution result。
- sandbox status metadata。

Runtime Events:
- PermissionRequest
- ToolCall
- ToolResult
- RuntimeError

Acceptance Tests:
- sandbox unavailable 返回 typed error 或明确 fallback，不静默裸跑。
- stdout/stderr/exit code 被完整保留。
- cleanup 失败会记录 warning event。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| App conversation API | bridge/remote 有基础 | stable local conversation server API 未固化 | `kiana-bridge/src`, `kiana-remote/src`, future app server | replay cursor and websocket fixtures | P2 |
| Event-to-UI reducer | TUI/stream-json 消费 events | UI reducer/golden fixtures 不完整 | `kiana-types/src/runtime.rs`, `kiana-screens/src` | RuntimeEvent to view-state snapshots | P1 |
| Sandbox provider | Bash sandbox/permissions 有基础 | provider boundary 和 unavailable fallback 规则不足 | `kiana-tools/src/bash_sandbox.rs`, `kiana-tools/src/permissions.rs` | local/windows/container readiness tests | P0 |
| Settings/secrets | auth/oauth/config command 基础存在 | secrets storage UX 和 server exposure policy 需明确 | `kiana-services/src/auth.rs`, `kiana-commands/src/config.rs` | no-secret-leak snapshots | P1 |

## 6. Atomic Implementation Tasks

### Task: Define Sandbox Provider Trait

Goal:
- 让本地 shell、Windows sandbox 和未来 container sandbox 通过同一执行边界接入。

Scope:
- 允许修改 `kiana-tools/src/bash_sandbox.rs`, `kiana-tools/src/bash_tool.rs`, `kiana-tools/src/powershell_tool.rs`。
- 不允许默认引入 Docker 依赖。

Implementation Notes:
- trait 输出 stdout、stderr、exit_code、sandbox_metadata。
- provider unavailable 必须显式返回。

Acceptance Criteria:
- local provider 与现有行为兼容。
- sandbox unavailable 不会静默绕过 restricted profile。
- ToolResult 包含 provider 名称和隔离状态。

Tests:
- `cargo test -p kiana-tools sandbox_provider`
- `cargo test -p kiana-tools powershell_policy`

Manual Verification:
- `kiana doctor sandbox`

### Task: Add RuntimeEvent View Reducer Fixtures

Goal:
- 证明 TUI/bridge/web shell 都能从同一 RuntimeEvent 序列生成一致 view state。

Scope:
- 允许新增 tests 到 `kiana-types` 或 `kiana-screens`。
- 不允许修改 RuntimeEvent 字段，除非同步 schema fixture。

Implementation Notes:
- 先覆盖 user/assistant/tool/error/final 五类事件。
- reducer 输出只包含 public UI state。

Acceptance Criteria:
- 相同 event 序列在 TUI/bridge fixture 中得到相同 tool status。
- tool error 不会吞掉 final result。
- unknown future event 被忽略或记录 warning。

Tests:
- `cargo test -p kiana-types runtime_view`
- `cargo test -p kiana-screens`

Manual Verification:
- `kiana --output-format=stream-json -p "run a failing command"`
