# roo-code — 源码补审

- **状态：** `audited`
- **参考路径：** `reference/Roo-Code`
- **主要运行时：** VS Code Extension Host + React Webview + `Task` Agent 循环
- **审计范围：** `src, packages, webview-ui, tool execution and task state`

本报告仅基于源码、测试、manifest 和可执行入口；未读取项目文档或 Git 历史。

## 端到端流程

```text
VS Code manifest / command
  → extension.activate()
  → ClineProvider
  ↔ WebviewMessage / ExtensionMessage
  → create / restore Task
  → context 与 system/tool assembly
  → provider model stream
  → presentAssistantMessage
  → validate / checkpoint / approval / tool execution
  → tool_result 回灌 API history
  → 下一模型步骤或完成
  → UI message/state delta 与持久化 history
```

## 入口与中央协调器

- `reference/Roo-Code/src/package.json:48-76` 指定扩展激活条件、`./dist/extension.js` 入口和 sidebar view；command 注册范围见 `:72-162`。
- `reference/Roo-Code/src/extension.ts:108-175` 的 `activate()` 初始化终端、配置迁移、i18n 和索引管理器，创建 `ClineProvider` 并注册 sidebar Webview。
- `reference/Roo-Code/src/core/webview/ClineProvider.ts:750-823,1238-1249` 建立 Webview 和消息监听，将消息交给统一 handler。
- `reference/Roo-Code/src/core/webview/webviewMessageHandler.ts:86-200,527+` 按 `message.type` 分派所有 Webview 输入。

## 新建、恢复与单活跃 Task

### 新建

- `reference/Roo-Code/src/core/webview/webviewMessageHandler.ts:601-623` 处理 `newTask`，解析图片 mention 后调用 `provider.createTask()`。
- `reference/Roo-Code/src/core/webview/ClineProvider.ts:2536-2626` 创建 Task；顶层 Task 在 `:2586-2593` 强制单 active task，并校验 provider/profile allow-list。
- Task 在加入 stack 后才调用 `start()`，避免运行开始时 UI 尚无 current task identity。
- `reference/Roo-Code/src/core/task/Task.ts:414-565,1892-1954` 构造 task/instance ID、API handler、ignore/protect controller、diff provider、队列和 token 统计；`startTask()` 清空 API/UI 历史、写入初始用户消息并进入 loop。

Roo 明确分离两类历史：

```text
apiConversationHistory
  → 模型协议历史

clineMessages
  → Webview 展示时间线
```

### 恢复

- `reference/Roo-Code/src/core/webview/ClineProvider.ts:825-1042,1695-1703` 读取历史 item 并调用 `createTaskWithHistoryItem()`。
- 恢复会处理 mode、固化的 provider profile、CLI runtime 配置优先级、旧 instance abort/dispose 和 pending edit 延迟提交。
- `reference/Roo-Code/src/core/task/Task.ts:1956-2187` 恢复持久化 API/UI history，清理旧 resume 提示和尾部 reasoning-only 消息。
- 恢复会补全没有对应结果的 native `tool_use`，生成 interrupted `tool_result`，防止 provider 因 tool-use/result 协议不闭合而返回 400。

## Agent Loop 与上下文组装

- `reference/Roo-Code/src/core/task/Task.ts:2427-2459` 的 `initiateTaskLoop()` 持续调用 `recursivelyMakeClineRequests()`；外层退出条件是 `!this.abort`。
- 若模型未使用工具且未完成，loop 会发送 no-tools-used 引导，促使模型继续或显式完成。
- `reference/Roo-Code/src/core/task/Task.ts:2461-2613` 每次请求前处理 provider rate limit、API started 事件、mentions、图片、workspace 文件、slash command、mode 切换和环境详情。
- 旧 `<environment_details>` 会被移除，避免恢复后动态上下文重复进入模型历史。

## 模型请求与流式输出

- `reference/Roo-Code/src/core/task/Task.ts:4103-4158` 根据 mode、custom mode、disabled tools 和模型能力构造 native tool 列表；metadata 包含 task ID、mode、自动 tool choice 和 parallel-tool-call 配置。
- `reference/Roo-Code/src/core/task/Task.ts:4160-4265` 每次请求创建独立 `AbortController`；首 chunk 使用 `Promise.race(firstChunk, abort)`，避免取消卡在网络首响应。
- context-window 错误有有限自动压缩/重试路径。
- `reference/Roo-Code/src/core/task/Task.ts:2771-2974` 消费 reasoning、usage、grounding、text、完整和 partial tool call；partial JSON 交给 `NativeToolCallParser` 增量解析。
- tool call 以 ID 建索引并拒绝重复 start，避免 duplicate `tool_use` 污染协议历史。

## 工具事务、审批与结果回灌

统一执行入口位于：

- `reference/Roo-Code/src/core/assistant-message/presentAssistantMessage.ts:59-950`

核心约束：

1. `presentAssistantMessageLocked` 防止流式回调重入导致并发工具执行（`:64-70`）。
2. 每个 native `tool_use` 必须恰有一个 terminal `tool_result`（`:405-488`）。
3. 缺 ID、malformed args、unknown/disabled/mode-restricted tool 都生成结构化错误结果，而不是留下悬空调用（`:295-318,408-440,560-610,825-889`）。
4. `askApproval()` 处理批准、拒绝和用户附加说明；一次拒绝后，本 assistant turn 后续工具会跳过，但仍为各自 ID 生成错误结果（`:491-526`）。
5. 文件、diff、terminal、MCP、follow-up、mode switch、subtask、completion、skill、image 等分发到具体 Tool class（`:651-823`）。
6. 写操作前调用 `checkpointSaveAndMark()`（`:651-823,957-967`）。

工具结果被写回 API history，随后由下一次模型请求读取，形成：

```text
assistant tool_use
  → validate
  → checkpoint
  → approval
  → execute
  → terminal tool_result
  → next model request
```

## Approval 状态机

- `reference/Roo-Code/src/core/task/Task.ts:1219-1454` 的 `ask()` 将请求写入 UI history，应用 auto-approval，支持超时回答，并通过 `pWaitFor()` 等待 UI response。
- 队列中的消息可以在审批时转化为“批准 + 用户反馈”。
- 等待超过阈值后，会按 ask 类型发送 interactive/resumable/idle 状态。
- `reference/Roo-Code/src/core/task/Task.ts:1456-1503` 处理 UI answer；用户消息回答会创建 checkpoint，工具批准会标记审批消息已答复。

## 取消、收敛与重新 hydration

- Webview cancel 入口：`reference/Roo-Code/src/core/webview/webviewMessageHandler.ts:1189-1191`。
- `reference/Roo-Code/src/core/webview/ClineProvider.ts:2629-2715` 的取消流程：读取历史、设置 `user_cancelled`、保存 instance ID、立即取消 HTTP stream、非阻塞启动 `abortTask()`、标记 abandoned、最多等待 3 秒，以 instance ID 双检后重新创建可恢复实例。
- `reference/Roo-Code/src/core/task/Task.ts:2212-2330` 设置 abort、刷新 token、取消 HTTP、移除 listeners、清理队列/terminal/command-output，并在编辑中断时尝试 revert diff。

这一设计保证 UI 取消后能回到可恢复 Task，但 `abortTask()` 非 await 与固定等待窗口使其仍是竞态敏感流程。

## Checkpoint、rewind 与编辑恢复

- 用户输入前 checkpoint：`reference/Roo-Code/src/core/task/Task.ts:1464-1469`。
- 写工具前 checkpoint：`reference/Roo-Code/src/core/assistant-message/presentAssistantMessage.ts:651-823,957-967`。
- UI 编辑/删除消息：`reference/Roo-Code/src/core/webview/webviewMessageHandler.ts:215-505`。
- 非 checkpoint 路径通过 `MessageManager.rewindToTimestamp()` 截断 UI/API history，再提交编辑后的用户消息。
- checkpoint restore 会取消并重建 Task，再通过 pending edit 向新 instance 注入修改：`reference/Roo-Code/src/core/webview/ClineProvider.ts:999-1040`。

## UI 事件与状态同步

- `reference/Roo-Code/src/core/webview/ClineProvider.ts:1828-1866` 支持完整 state、忽略 task history 的 state、忽略 history 和 `clineMessages` 的 state。
- omit-message 变体用于避免异步旧快照覆盖流式期间更新的消息。
- `reference/Roo-Code/webview-ui/src/context/ExtensionStateContext.tsx:276-426` 用 `mergeExtensionState()` 合并状态；`messageUpdated` 通过 timestamp 定位，未知 timestamp 会丢弃并告警。
- task history 使用独立 delta；Chat UI 处理 send/new chat、condense、checkpoint warning、interaction required 和成本显示：`reference/Roo-Code/webview-ui/src/components/chat/ChatView.tsx:838-939`。

## 测试覆盖

主要测试目录：

- `reference/Roo-Code/src/core/task/__tests__/`
- `reference/Roo-Code/src/core/assistant-message/__tests__/`
- `reference/Roo-Code/src/core/checkpoints/__tests__/`
- `reference/Roo-Code/src/core/task-persistence/__tests__/`

可见覆盖包括：

- queued message drain；
- duplicate tool-use IDs；
- pending tool-result flush；
- API stream retry；
- native tool filtering；
- partial tool parser；
- malformed/unknown custom tool；
- task persistence/dispose；
- provider profile race；
- checkpoint 保存、取消和恢复。

## 优点

- 模型协议历史与 UI 时间线分离。
- 恢复时主动修复未闭合的 native tool transaction。
- 工具处理集中保证“每个 tool_use 恰有一个结果”。
- 写操作和用户反馈前有 checkpoint。
- 模型流、工具 partial JSON 和 UI 增量均有专门处理。
- top-level 单 active Task 和 instance ID 双检降低生命周期竞态。

## 风险与缺口

### 高：Webview 输入缺乏统一运行时 schema 校验

`reference/Roo-Code/src/core/webview/webviewMessageHandler.ts:86-90,527+` 大量依赖 TypeScript 类型与 `!` 非空断言。Webview 是真实进程边界，损坏或恶意 payload 可能导致异常、历史错位、配置污染或非预期工具控制流。

建议所有 action 使用 discriminated-union runtime schema，尤其覆盖：

```text
newTask
askResponse
cancelTask
task ID
edit/delete timestamp
settings
command allow/deny list
```

### 高：取消、恢复与 dispose 的竞态

`ClineProvider.ts:2667-2668` 非 await 调用 `abortTask()`，再通过 3 秒窗口和 instance ID 重建。重复 cancel、checkpoint restore、history open 和 view dispose 交叠时仍可能竞态。

建议用单调 lifecycle generation/epoch 和 transition lock 串行化：

```text
cancel
restore
resume
dispose
rehydrate
```

并增加 first chunk、partial tool JSON、approval、checkpoint restore 和连续取消的 E2E race matrix。

### 中：异步 state snapshot 仍可能陈旧覆盖

虽然 `ClineProvider.ts:1852-1866` 已提供 omit-message 缓解，状态快照、message delta、task-history delta 和多 listener 仍依赖隐含排序。

建议每个 UI delta 带：

```text
taskId
instanceId
generation
sequence
```

并由 reducer 丢弃旧 generation/sequence。

### 中：工具处理函数职责过密

`presentAssistantMessage.ts:59-950` 同时承担解析、审批、错误、checkpoint、执行、completion、custom tool 和 MCP。新增工具容易让安全路径分叉。

建议抽象：

```text
ToolExecutionTransaction
  = validate
  → checkpoint
  → approval
  → execute
  → normalizeResult
  → persistResult
```

## 对 Kiana 的映射

| Roo 概念 | Kiana 建议抽象 |
|---|---|
| `ClineProvider` | `AgentRuntimeCoordinator`：唯一 task registry、生命周期 gate、UI/transport bridge |
| `Task` | `AgentSession`：不可变 ID + 单调 generation + event-sourced state |
| `apiConversationHistory` / `clineMessages` | `ModelTranscript` 与 `PresentationTimeline` 分离 |
| `presentAssistantMessage` | `ToolOrchestrator` + `ToolExecutionTransaction` |
| `ask()` | `ApprovalBroker`：request ID、timeout、queue、disconnect policy、audit |
| 每 turn `AbortController` | model-turn cancel token + session parent token |
| checkpoint | `CheckpointService`：绑定 transcript offset、workspace revision 和 tool transaction |
| `clineMessagesSeq` | `SessionEvent.sequence`，所有 UI event 带 session/generation/seq |
| `createTaskWithHistoryItem()` | `rehydrateSession(snapshot, policy)`，补全未结束 tool transaction |

Kiana 首期应建立以下 invariant：

1. 一个 Session 内只有一个 active model turn。
2. 每个 tool call 有全局唯一 ID。
3. 每个开始的 tool call 恰有一个 terminal result：success、denied、cancelled、invalid 或 failed。
4. UI event 一律带 session ID、generation 和 sequence。
5. cancel/restore/resume 只能通过 lifecycle gate。
6. Model transcript、tool transaction log 和 UI timeline 不共享同一个可变数组。
7. checkpoint 绑定 transcript offset、workspace revision 和 active invocation。
8. UI 输入和 approval response 一律运行时 schema validation。
