# openhands — 源码补审

- **状态：** `audited`（前端/适配层边界）
- **参考路径：** `reference/OpenHands`
- **主要运行时：** `@openhands/agent-canvas` React/TypeScript 前端、CLI/Electron 启动器与 Agent Server 适配层
- **审计范围：** `src, runtime adapters, event/state handling, UI, tests`

本报告仅基于源码、测试、manifest 和可执行入口；未读取文档或 Git 历史。

## 审计边界

当前参考目录不是 OpenHands Python Agent Server 本体。`reference/OpenHands/package.json:1-28` 将包命名为 `@openhands/agent-canvas`，并依赖外部 `@openhands/typescript-client`。

可直接验证：

- UI/浏览器 ingress；
- conversation 创建参数；
- REST/WebSocket 事件桥；
- 前端状态、确认调用、停止/恢复；
- UI metadata 持久化和输出投影。

无法从当前源码验证：

- 服务端 controller/agent loop；
- LLM provider 请求实现；
- 服务端工具沙箱与策略强制；
- 服务端事件持久化/恢复；
- tool observation 如何进入模型上下文。

## 总体流程

```text
浏览器路由 / Chat 输入
  → ConversationWebSocketProvider.sendMessage()
  → Local SDK 或 Cloud runtime REST
  → 外部 Agent Server
     → controller / model / tools / sandbox
     → persisted events + live events
  → Canvas WebSocket handler
  → raw/UI event store
  → terminal/browser/metrics/state 副作用
  → React Conversation UI
```

## 入口与 Conversation 创建

- CLI binary 为 `agent-canvas`：`reference/OpenHands/package.json:16-18`。
- 开发与 Electron 入口见 `reference/OpenHands/package.json:74-80,108-114`。
- 对话路由读取 URL conversation ID，并挂载 `WebSocketProviderWrapper`：`reference/OpenHands/src/routes/conversation.tsx:33-55,194-215`。
- backend/org 切换时拒绝挂载旧 conversation，避免 ID 发往错误 backend：`:180-191`。
- Cloud 创建向 `/api/v1/app-conversations` 提交初始消息、仓库/分支、plugins、parent、sandbox、profile：`reference/OpenHands/src/api/conversation-service/agent-server-conversation-service.api.ts:420-446`。
- Local 创建生成 UUID，解析绝对工作目录，组装 settings，通过 SDK `createConversation()`：`:449-491`。
- 默认 local workspace 为 `workspace/project/<conversation-id>`：`reference/OpenHands/src/api/agent-server-config.ts:196-207`。

## 状态模型

Canvas 接受：

```text
idle
running
paused
waiting_for_confirmation
finished
error
stuck
```

未知状态退化为 `idle`：`reference/OpenHands/src/api/conversation-service/agent-server-conversation-service.api.ts:306-323`。

服务端 `ConversationStateUpdateEvent` 更新 execution status、stats、metrics 和 goal store：`reference/OpenHands/src/contexts/conversation-websocket-context.tsx:637-655`。

前端 localStorage 仅保存 UI metadata：repository/branch、workspace、active profile 和不含参数的 plugin 坐标；不保存 runtime 轨迹或 plugin secrets：`reference/OpenHands/src/api/conversation-metadata-store.ts:4-92`。

## 请求、模型流与工具事件

- Local `sendMessage()` 调 SDK `sendEvent(..., {run:true})`；Cloud POST `{...message, run:true}`：`reference/OpenHands/src/api/conversation-service/agent-server-conversation-service.api.ts:356-401`。
- OpenHands agent 配置强制 `llm.stream=true`，否则服务端不发 `StreamingDeltaEvents`：`reference/OpenHands/src/api/agent-server-adapter.ts:883-897`。
- stream delta 按 animation frame 批处理，降低 React/store 写入压力：`reference/OpenHands/src/contexts/conversation-websocket-context.tsx:162-180,547-555`。
- delta 不进入 `eventIds`，连续同 sender delta 合并；带 ID 的普通事件去重：`reference/OpenHands/src/stores/use-event-store.ts:92-129`。

默认服务端 toolset：

```text
terminal
file_editor
task_tracker
browser_tool_set（可选）
task_tool_set（sub-agent 可用时）
```

证据：`reference/OpenHands/src/api/agent-server-adapter.ts:113-119,620-677`。

前端注册 Canvas UI tool 与 child-conversation tool：`:1085-1120`。

事件投影包括：

- Bash action/observation → terminal；
- Browser observation → screenshot；
- Browser navigate → URL；
- Canvas UI action → 本地 UI；
- child conversation action → 浏览器创建子会话并反馈结果。

位置：`reference/OpenHands/src/contexts/conversation-websocket-context.tsx:626-740`。

## WebSocket、历史与重连

- WebSocket 打开后发送 session API key auth：`reference/OpenHands/src/utils/websocket-auth.ts:1-18`。
- socket hook 提供 watchdog、错误状态和 1–30 秒指数退避重连：`reference/OpenHands/src/hooks/use-websocket.ts:18-20,56-74,110-139,200-248`。
- 初始历史经 REST 拉取最近 50 条并转为正序：`reference/OpenHands/src/hooks/query/use-conversation-history.ts:7-18,48-71`。
- WS 等待初始历史完成，再以最新 timestamp + `resend_mode='since'` 增量订阅：`reference/OpenHands/src/contexts/conversation-websocket-context.tsx:278-291,359-400`。
- 重放事件在执行 terminal/browser/cache/UI 副作用前去重，避免重复副作用：`:556-567`。
- Store 维护 raw events、UI events、event IDs 和 loadedConversationId，并按 timestamp 修正乱序：`reference/OpenHands/src/stores/use-event-store.ts:55-90,131-223`。

## Approval 与 Secret 边界

前端将设置映射为服务端 confirmation policy：

```text
confirmation_mode=false → NeverConfirm
LLM analyzer             → ConfirmRisky(HIGH, confirm_unknown=true)
其他 confirmation        → AlwaysConfirm
```

位置：`reference/OpenHands/src/api/agent-server-adapter.ts:593-618`。

用户通过 `/api/conversations/{id}/events/respond_to_confirmation` 批准或拒绝：`reference/OpenHands/src/api/event-service/event-service.api.ts:39-69`、`reference/OpenHands/src/hooks/mutation/use-respond-to-confirmation.ts:1-32`。

前端只能证明策略和 response 被传递，不能证明 Agent Server 在工具执行前强制审批、绑定原 action、阻止 replay 或跨 conversation 复用。

Session API key 来源于 env 或 `window.__AGENT_CANVAS_SESSION_API_KEY__`，HTTP 使用 `X-Session-API-Key`：`reference/OpenHands/src/api/agent-server-config.ts:102-131,209-212`。浏览器可读 key 必须依赖短寿命、窄权限、同源/CSP 等后端保障，当前仓库无法验证。

Secret 处理：subscription 模式移除 LLM api key/base URL；自定义 secret 使用 `LookupSecret`，plugin metadata 不保存参数：`reference/OpenHands/src/api/agent-server-adapter.ts:899-921,1147-1159,1203-1227`。

## Stop、Cancel、Resume 与 Goal

- Local stop 调 `/interrupt`，意图立即取消 LLM：`reference/OpenHands/src/hooks/mutation/conversation-mutation-utils.ts:36-61`。
- Cloud stop 暂停 sandbox，可能等待当前 LLM call 完成，并非硬取消：`:36-53`。
- Local resume 调 `runConversation()`：`:117-123`。
- `stopGoal()` 只停止 goal loop，不停止 in-flight Agent turn；彻底停止还需 pause/interrupt：`:77-115`。
- Cloud sandbox 为 PAUSED 时不连旧 WebSocket，恢复后重新获取 runtime URL：`reference/OpenHands/src/contexts/websocket-provider-wrapper.tsx:24-45`。

## Persistence、Fork 与 Compaction

- Cloud history 位于 Cloud App API；runtime control 位于 per-conversation endpoint：`reference/OpenHands/src/api/event-service/event-service.api.ts:19-37`。
- Local history 用 `RemoteEventsList.search()`；Cloud 用 event search API：`:97-180`。
- `/condense` 触发 context compaction：`reference/OpenHands/src/api/conversation-service/agent-server-conversation-service.api.ts:717-745`。
- Local 支持按 event ID fork，Cloud 明确拒绝：`:792-825`。
- trajectory 导出分别走 Cloud download 或 `FileClient.downloadTrajectory()`：`:673-681`。

## 测试覆盖

Manifest 提供 Vitest、Playwright、live/mock-LLM E2E、Docker E2E、coverage 和 mutation testing：`reference/OpenHands/package.json:84-91`。

相关目录：

- `reference/OpenHands/__tests__/api/event-service/`
- `reference/OpenHands/__tests__/api/runtime-service/`
- `reference/OpenHands/__tests__/hooks/chat/`
- `reference/OpenHands/__tests__/components/conversation-events/`
- `reference/OpenHands/tests/e2e/mock-llm/`
- `reference/OpenHands/tests/e2e/live/`
- `reference/OpenHands/tests/e2e/live-acp/`

## 优点

- REST tail preload + since WebSocket 形成可恢复事件桥。
- 事件 ID 去重和副作用前去重避免重放造成重复 UI/terminal 行为。
- stream delta 帧批处理降低渲染负担。
- conversation 切换防止跨 backend 串台。
- runtime session key 与 Cloud App auth 分离。
- UI metadata 明确不作为完整 runtime state。

## 风险与限制

1. **当前仓库不是完整 Agent Server。** 无法验证模型、tool executor、sandbox、服务端 persistence 和 approval enforcement。
2. **停止语义不对称。** Local interrupt、Cloud pause、Goal stop 含义不同，调用者不能统一解释为“已取消”。
3. **Session key 前端可见。** 必须由后端提供短 TTL、窄 scope 和撤销保障。
4. **审批强制属于服务端。** 前端策略与 response 不是可信执行边界。
5. **UI metadata 不是授权事实。** localStorage repository/profile/workspace 不能用于服务端权限判断。
6. **外部 SDK 版本是审计依赖。** `@openhands/typescript-client` 的具体协议和行为需单独审计。

## 对 Kiana 的映射

- Canvas 可参考为薄 UI/runtime adapter，而不是 Agent 核心。
- Kiana Web 应采用 snapshot/tail preload + cursor event stream，并在副作用前去重。
- Local/remote cancel、pause、stop-goal 必须定义不同 terminal/paused 状态。
- Approval policy 必须在 `ControlPlane/CapabilityBroker` 强制，UI 只提交 decision。
- Session token 必须进程级/会话级窄 scope，不能依赖 loopback。
- UI metadata、runtime state、authorization state 必须分别持久化。
- 外部 SDK/服务端缺失时，报告必须明确 audit boundary，不能从前端推断工具安全。
