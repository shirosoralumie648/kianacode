# cline — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/cline`
- **主要运行时：** cline
- **审计范围：** `apps, core agent loop, tools, task persistence, webview/extension bridge`

本报告基于源文件、测试、清单和可执行入口。

## 端到端流程

- 1. VS Code 激活扩展并创建 Controller/WebviewProvider。
- 2. React webview 针对具体用户提示发送 gRPC NewTaskRequest。
- 3. VscodeWebviewProvider -> grpc-handler -> newTask 规范化设置并调用 Controller.initTask。
- 4. Controller 设置流式 UI 阶段；SdkTaskStartCoordinator 清除此前会话，解析工作区/模式/提供商/模型/认证，分配或恢复会话 ID，发出乐观的任务消息并创建 TaskProxy。
- 5. SdkSessionLifecycle 启动 VscodeSessionHost；VscodeSessionHost 准备 ClineCore 本地运行时、策略、审批/执行器回调、远程配置和 VS Code 工具。
- 6. ClineCore/LocalRuntimeHost 创建或恢复 SQLite/manifest/message 工件，构建 DefaultRuntimeBuilder 工具和 SessionRuntime，并订阅事件桥。
- 7. 即发即忘的发送进入 LocalRuntimeHost.runTurn；提及/模式/文件被规范化，会话随后标记为运行中，SessionRuntime 追加用户内容，AgentRuntime 生成模型请求。
- 8. 提供商模型流发出增量；AgentRuntime 组装助手内容和工具调用。在工具执行前，它应用 hooks、启用/自动审批策略和主机审批回调。
- 9. 已批准的工具使用 AbortSignal 执行；输出变成工具结果消息并追加到运行时历史。while 循环请求下一轮模型调用，并在需要时注入待处理的用户/hook 上下文。
- 10. 无工具响应、成功的终端完成工具、最大迭代次数超限、提供商错误、循环/错误停止或取消会终止运行。SessionRuntime 转换运行时事件并返回 AgentResult。
- 11. LocalRuntimeHost 持久化转录/用量/元数据，更新空闲/已完成/失败/已取消状态，并在适当时排空排队的提示。
- 12. SdkMessageCoordinator 将 CoreSessionEvents 转换为带 seq/epoch 的 ClineMessages；WebviewGrpcBridge 推送部分消息和终端状态流；React 渲染最终的助手/工具/错误输出。

## 组件与边界

- 主要运行时：VS Code 扩展 + @cline/core/@cline/agents 本地 SDK 路径。入口清单：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/package.json:42-49。扩展激活、存储迁移、初始化：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/extension.ts:62-86。
- 入口和 UI 桥：webview 在 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/webview-ui/src/components/chat/chat-view/hooks/useMessageHandlers.ts:34-47,167-246 中发送 NewTaskRequest/AskResponseRequest；VS Code 在 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/hosts/vscode/VscodeWebviewProvider.ts:155-190 和 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/core/controller/grpc-handler.ts:52-103 中接收并分派 gRPC。
- 任务/会话生命周期：请求处理器 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/core/controller/task/newTask.ts:15-73 和 askResponse.ts:13-45；控制器阶段转换 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/SdkController.ts:1384-1512；新建/恢复任务编排 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/sdk-task-start-coordinator.ts:63-215。
- 主机/运行时构造：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/sdk-session-lifecycle.ts:143-180,251-260 和 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/vscode-session-host.ts:114-220,227-287。ClineCore start/send/abort/stop 委托：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/ClineCore.ts:284-343,364-417。
- 核心编排：每个会话的转录、跟踪器、监听器、运行状态位于 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/runtime/orchestration/session-runtime-orchestrator.ts:303-356；运行/继续/中止/关闭和认证重试位于 :590-767；上下文/模型/工具设置以及完整转录替换位于 :769-965；消息构建器和 API 安全准备位于 :1006-1103。
- agent 循环和提供商流：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/agents/src/agent-runtime.ts:689-937 处理运行迭代、完成、最大迭代、错误和中止；:1020-1113 构建每个模型请求并打开流；:1130-1384 组装文本/推理/媒体/工具调用增量和用量；:1387-1435 记录提供商流生命周期和失败。
- 策略/工具执行：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/agents/src/agent-runtime.ts:1629-1769 处理顺序/并行调用、hook 变更、启用策略和审批门；:1772-1917 调用审批回调和工具执行器，捕获错误并发出工具结果；循环/错误执行位于 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/runtime/orchestration/session-runtime-orchestrator.ts:1105-1241,1283-1359。
- 持久化/恢复：SQLite 会话行位于 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/session/services/session-service.ts:19-71,73-108,110-221；工件/manifest/message 初始化和写入位于 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/session/services/persistence-service.ts:102-176,309-333；本地主机会话创建/恢复和持久化种子的历史位于 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/runtime/host/local-runtime-host.ts:394-520,913-951；轮次持久化/错误/中止处理位于 :1039-1109,1667-1731,1767-1839,1841-1959。
- 事件/UI 输出：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/sdk-message-coordinator.ts:19-100 标记 seq/epoch、存储消息并扇出事件；/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/webview-grpc-bridge.ts:32-86,89-148 发送部分消息/状态更新；/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/task-proxy.ts:167-262 提供任务代理兼容性。
- 次要运行时：CLI 入口/运行时位于 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/cli/src/main.ts 和 runtime/run-agent.ts、runtime/run-interactive.ts；Hub 服务器位于 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/cline-hub/src/server/sessions.ts 和 agent-events.ts；共享 SDK 包入口位于 sdk/packages/core/package.json:12 和 sdk/packages/agents/package.json:9。

## 状态、持久化与恢复

- 会话状态包括 idle/running/pending 以及终态 completed/failed/cancelled；交互式会话在一轮之后返回 idle，而单次运行会话会关闭。
- 运行时状态跟踪对话消息、待处理工具调用、用量、运行/迭代、终止状态、错误和循环跟踪器。
- webview 会在权威的带标记任务/用户消息到达前，维护乐观的待处理用户消息。

## 工具、策略与副作用

- Read
- Bash
- Skill(claude-api)
- StructuredOutput

## 输出与呈现

- 审计了主要 VS Code 运行时的仅源码实现流程。
- 引用的跟踪中未使用被排除的 README/CLAUDE/AGENTS/USER/docs/变更日志/git-history 材料。
- 实现通过 TaskProxy 有意保留旧版 webview/task 接口，同时将实际执行委托给 ClineCore 和 AgentRuntime。

## 测试与验证

- Agent runtime：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/agents/src/agent-runtime.test.ts（工具循环、审批、无效 JSON、并行工具、完成、溢出、取消、hooks）。
- Core orchestration：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/runtime/orchestration/session-runtime-orchestrator.test.ts（消息准备、运行/继续、事件扇出、中止/重入）。
- Local host：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/runtime/host/local-runtime-host.test.ts（会话创建/恢复、持久化、状态、队列、中止、认证重试、压缩）。
- Extension：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/sdk-task-start-coordinator.test.ts；/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/sdk-task-resume.test.ts；/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/webview-grpc-bridge.test.ts。

## 优势

- 循环通过从 ConversationStore 为每次 AgentRuntime 运行提供种子，并用完整返回转录替换存储，从而保留完整的多轮历史。
- 流式解析保留文本、推理、媒体和工具调用增量之间的顺序，并将格式错误的工具输入标记为工具错误，而不是让整个轮次崩溃。
- 审批和工具策略集中管理，可由 hook 覆盖，并针对拒绝/覆盖行为进行了测试。
- 中止处理异常审慎：观察预期的 promise rejection，转发 AbortController 取消，保留排队提示，刷新已中止的转录，并等待跟踪器工作完成。
- 持久化具有针对种子会话、恢复、压缩、状态锁定、队列行为以及轮次/错误/中止转录持久性的明确恢复测试。
- 扩展桥维护向后兼容的任务/消息接口，同时向现有 gRPC/webview 消费者暴露 SDK 事件流。

## 风险与缺口

- VS Code 适配器负担了大量兼容性：TaskProxy 明确为浏览器和终端 API 暴露存根（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/task-proxy.ts:127-147,243-250），因此依赖已移除的经典行为的调用者可能会静默空操作。
- 事件传递使用独立的部分消息和完整状态通道；正确性依赖 seq/epoch 标记和 webview 协调（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/sdk-message-coordinator.ts:26-41）。
- 核心持久化在活动轮次期间会有意滞后，并需要在助手边界、错误和中止时显式刷新（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/runtime/host/local-runtime-host.ts:1476-1501,1767-1807）；失败会被记录/发送遥测，通常不会替换用户可见的运行错误。
- 审批拒绝被表示为 isError 工具结果，循环继续，因此策略拒绝会变成模型可见的工具失败，而不是硬性会话停止（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/agents/src/agent-runtime.ts:1744-1761,1812-1905）。
- 没有工具调用的提供商流失败会作为 AgentResult 错误呈现；类似认证的失败可能会触发一次主机凭据刷新/重试，然后仍以失败结束（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/runtime/orchestration/session-runtime-orchestrator.ts:738-767）。

## 映射到 Kiana

- 入口 -> VS Code webview gRPC -> 任务处理器
- 会话/线程 -> SdkTaskStartCoordinator + SdkSessionLifecycle + ClineCore 会话 ID
- 上下文 -> 提及/模式/文件规范化 + ConversationStore + MessageBuilder
- 模型 -> AgentRuntime AgentModel.stream 提供商事件流
- 工具 -> 流式工具调用组装 + 策略/审批 + 执行器 + 工具结果注入
- 终止 -> 完成守卫、终端完成工具、最大迭代次数、中止/错误、循环/错误安全机制
- 持久化 -> SQLite 会话 + manifest + JSON 消息 + 压缩 sidecar
- 输出 -> CoreSessionEvent -> SdkMessageCoordinator -> WebviewGrpcBridge -> React 聊天
- 次要运行时 -> CLI 运行时和 Hub 服务器复用核心主机

## 源码证据

- 追踪的具体请求：webview 文本 `inspect this file` -> NewTaskRequest -> newTask handler -> SdkController.initTask -> 配置/会话 ID -> VscodeSessionHost/ClineCore.start -> SdkTaskStartCoordinator.fireAndForgetSend -> LocalRuntimeHost.runTurn -> SessionRuntime.continue/run -> AgentRuntime 模型流 -> 流式工具调用 -> 策略/审批 -> 工具执行器 -> 工具结果注入下一次迭代 -> 最终助手结果 -> 消息持久化/状态 -> SDK 事件转换 -> gRPC 部分/状态流 -> React 聊天。
- 提示/上下文组装证据：SdkTaskStartCoordinator 在发送前解析上下文提及（:151-155）；LocalRuntimeHost.prepareTurnInput 格式化模式并合并显式/被提及文件（:1998-2047）；SessionRuntime 追加用户内容，并在 createAgentRuntimeConfig 前注入完整此前对话（:808-899）；AgentRuntime 请求包括系统提示、消息、工具、信号和模型选项（:1020-1071）。
- 工具解析证据：AgentRuntime 消费文本/推理/媒体/工具调用增量，组装分片输入，并在助手元数据中标记无效 JSON（:1130-1353），然后发出规范工具调用消息并执行它们（:815-848,1629-1917）。
- 终止证据：除非完成守卫添加用户提醒，否则无工具响应会完成（:790-812）；成功的 completesRun 工具立即结束（:849-865）；maxIterations/error/abort 会被规范化（:869-937）；核心主机将完成原因映射到持久化会话状态（:2160-2173）。
- 取消/错误证据：控制器在中止前为可恢复状态设置栅栏（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/apps/vscode/src/sdk/SdkController.ts:1406-1412）；AgentRuntime 通过 AbortController 中止提供商流（:543-559）；LocalRuntimeHost 保留排队提示并刷新已中止转录（:1121-1141,1767-1839）；gRPC 处理器返回序列化错误（:92-103,143-153）。
- 持久化证据：根会话创建写入 SQLite 行、manifest 和 messages 工件（:102-176）；已完成轮次持久化消息、用量和元数据（:1886-1923）；失败会尝试刷新转录而不掩盖原始错误（:1924-1952）。
- 测试覆盖该追踪：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/agents/src/agent-runtime.test.ts:676 工具循环，1152 审批，2183 hook 阻断，2363 格式错误的工具 JSON，2553 并行执行，1829/1880/1923 取消；/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/cline/sdk/packages/core/src/runtime/host/local-runtime-host.test.ts:1128 种子恢复，1609 持久化/状态，1688 刷新失败，2816/3052 中止队列行为，4232 恢复，6306 认证重试；扩展测试位于 sdk-task-start-coordinator.test.ts:26-228、sdk-task-resume.test.ts:32-60、webview-grpc-bridge.test.ts:40-128。
