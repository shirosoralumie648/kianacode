# crush — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/crush`
- **主要运行时：** 项目键 crush；仅审计 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush 源码树，排除 README/CLAUDE/AGENTS/USER/docs/changelogs/git history。
- **审计范围：** `internal, command entrypoints, agent, tools, sessions and UI`

本报告基于源文件、测试、清单和可执行入口。

## 端到端流程

- 具体请求：`crush run "inspect the project"` 进入 runCmd，合并参数/stdin，安装信号上下文，选择服务器或本地工作区，确保提供商和 agent 就绪，然后解析显式/最近/新的顶层会话。
- 服务器模式在 POST 提示前订阅工作区 SSE。POST 验证工作区/协调器，验证非空提示/会话，预留 AcceptedRun，增加工作区运行等待组，启动工作区上下文 goroutine，并返回 HTTP 202。
- Coordinator 在非交互模式下等待 MCP 初始化，刷新模型/工具配置，解析提供商选项和模型参数，传递 RunID 和认证刷新回调，并调用 SessionAgent.Run。
- SessionAgent 将分派与 Cancel 序列化：已接受请求会在进入时取消，在活动会话之后排队，或在创建助手前注册为活动请求。它读取持久化消息，为第一个文本提示启动分离的标题生成，持久化用户消息，在前面添加 todo reminder/system prompt/MCP instructions，转换历史和附件，并修复孤立的工具调用/结果。
- Fantasy Agent.Stream 将组装后的提示/历史/文件/工具发送给已配置的提供商。PrepareStep 刷新工具并折叠符合条件的排队提示。推理/文本/工具输入/工具调用/工具结果回调更新助手/工具行；工具结果以 role=tool 持久化，并反馈给 Fantasy 的下一步骤。
- 每个工具都会根据 agent AllowedTools/AllowedMCP 进行过滤。带钩子的工具先执行 PreToolUse；deny 返回错误响应，halt 设置 StopTurn，输入重写会被应用，allow 则记录每工具调用的权限批准。否则 `permission.Request` 应用 skip/allowlist/session 授权，或发布 PermissionRequest 并阻塞，直到 UI Grant/Deny/上下文取消。Bash 还会分类安全的只读命令、应用命令阻止器、为不安全命令请求执行权限，并同步运行或作为后台作业运行。
- OnStepFinish 持久化完成原因和用量/成本；StopWhen 可以在接近上下文限制时触发摘要，或停止重复的工具调用循环。错误/取消会关闭未完成的工具调用，注入错误/已取消的工具结果，标记助手完成状态，并使用分离的有界清理上下文持久化。成功会释放活动状态，发出完成通知，并原子地排空/递归处理排队轮次。
- Agent RunComplete 在 FlushAll 后发出。Coordinator 合并认证重试尝试，并且只保证传递权威的终端事件。若失败发生在 SessionAgent 发布事件之前，Backend 会可靠地补发 RunComplete。SSE 客户端解码消息/会话/权限/问题/agent/run-complete 事件。存在 RunID 时，`crush run` 忽略实时助手增量，等待匹配的 RunComplete，然后协调最终嵌入的 Text，以覆盖乱序/丢失的消息更新并写入 stdout。
- 交互式 TUI 在第一次发送时创建会话，乐观地标记 busy，并通过 Workspace.AgentRun 发送。ClientWorkspace 使用 HTTP 202；AppWorkspace 在进程内直接调用 Coordinator.Run。Pubsub 消息更新会渲染助手/工具输出；权限/问题事件打开对话框；会话和 busy/queue 事件更新侧栏/状态；Escape 在 UI 的双击保护后路由到取消。

## 组件与边界

- 主要运行时：Go 1.26 CLI，使用 Bubble Tea TUI，可选的客户端/服务器 Unix-socket HTTP API，Fantasy 模型/工具编排，SQLite 持久化。
- 次要路径：本地进程内 AppWorkspace 绕过 HTTP；服务器模式使用 ClientWorkspace + HTTP/SSE。两者都汇聚到 Coordinator 和 SessionAgent。

## 状态、持久化与恢复

- 会话状态：SQLite 支持的 sessions/messages/files，加上内存中的估算用量标记和 pubsub 更新。
- Agent 状态：每会话的活动取消条目、排队调用、已接受预留、取消高水位标记、可变模型/系统提示/工具快照。
- 流状态：当前助手消息、流式增量/推理/工具调用、每步骤历史快照、用量/成本、摘要/循环标志。
- UI 状态：当前会话/聊天项目、乐观和权威的 busy/queue 缓存、权限/问题对话框、事件驱动渲染。

## 工具、策略与副作用

- Read、Bash、grep/find，用于仅源码导航；未执行文件修改。

## 输出与呈现

- Go 实现提供了完整且可由源码跟踪的请求生命周期，分离的本地和服务器入口路径最终汇聚到 Coordinator/SessionAgent。
- 会话身份持久化在 SQLite 中；续接会拒绝子会话，恢复会话可以还原上一个助手仍可用的模型/提供商。
- 取消围绕已接受运行预留、每会话分派锁、高水位取消标记、队列丢弃、分离的清理写入以及明确的已取消终端事件设计。
- 流式输出以增量方式持久化，并通过防抖的消息更新处理；终端 RunComplete 携带最终文本，以协调事件顺序和订阅者丢失。
- 工具具备策略感知：允许工具/MCP 过滤、PreToolUse 钩子决策、权限提示/授权、shell 命令阻止器、后台作业和 StopTurn 语义都在源码中有所体现。
- 测试直接覆盖已接受/取消竞态、队列/运行 ID 行为、流关联、端到端取消/分离、钩子批准、权限解析、事件转换、会话持久化和消息刷新顺序。

## 测试与验证

- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/accepted_run_test.go:40-227 — 预留、进入时取消、已取消持久化。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/dispatch_cancel_test.go:73-398 and dispatch_race_test.go:113- — 活动/已接受取消和并发分派。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/run_complete_test.go:21-312 and queued_runid_test.go:78- — 队列过滤、必须传递、已取消/运行 ID 终端事件。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/agent_test.go:662-896 — 附件和孤立工具历史修复。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/hooked_tool_test.go:49- — 钩子 allow/silent/deny 批准行为。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/cmd/run_stream_test.go:20-426 — 工具使用不终止、RunComplete、乱序协调、RunID 过滤、错误/取消。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/server/e2e_agent_test.go:324-668 — 跨客户端取消、立即取消、分离请求上下文、工作区声明。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/server/agent_cancel_test.go:132-190 and events_test.go:96-164 — HTTP 取消/分离和事件转换。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/session/session_test.go:10-78 and message/message_test.go:94-671 — 用量标记、消息防抖、终端刷新、在途写入顺序。

## 优点

- 围绕已接受到活动状态移交和取消竞态的并发设计稳健，并有专门测试。
- RunID 关联防止排队/并发会话轮次终止错误的无交互调用者。
- 终端事件发布刻意排在消息 FlushAll 之后，并携带最终文本以便协调。
- 提示历史修复会处理中断后的孤立工具调用/结果，防止提供商验证死锁。
- 工具和钩子策略明确且可组合，并具有原子权限解析和持久化授权竞态保护。
- 提供商抽象支持多种 API 系列、认证刷新、重试、推理元数据以及会话续接时的模型恢复。

## 风险与缺口

- 本地 AppWorkspace.AgentRun 是同步的，而服务器 ClientWorkspace.AgentRun 是即发即忘的；尽管二者共享 agent 核心，UI 命令 worker 的行为在不同运行时之间仍有差异。
- Pubsub 存在有损的普通事件路径，因此实现依赖必须传递的 RunComplete 及其中嵌入的最终 Text 来完成非交互终止；Coordinator 发布前的失败需要 Backend 回退。
- 非交互运行可用的 MCP 工具取决于有界初始化就绪；交互式运行会有意使用当前已注册的工具表，并在工具列表之后发生变化时获取更新。
- 提供商/认证/模型配置会动态重建，并存在许多特定提供商的选项分支，因此不同提供商和模型系列的行为不同。
- 分离的清理上下文会在取消/工作区请求上下文之后有意继续存在，最长不超过有界超时；关闭顺序必须在此窗口内保持数据库/服务可用。

## 与 Kiana 的映射

- 入口：cmd/run.go、ui/model/ui.go、workspace/*、server/proto.go、client/proto.go。
- Agent 编排：agent/coordinator.go 和 agent/agent.go。
- 上下文/提示组装：agent/agent.go 的 preparePrompt 和 PrepareStep；agent/prompts.go 及嵌入式模板。
- 模型流：agent/agent.go 中 Fantasy Agent.Stream 回调。
- 工具/策略：agent/coordinator.go、agent/hooked_tool.go、agent/tools/*、permission/permission.go。
- 持久化/恢复：session/session.go、message/message.go、db/*、agent 取消/孤立修复路径。
- 事件/UI：server/events.go、server/proto.go SSE、client/proto.go、ui/model/ui.go、ui/chat/*。

## 源码证据

- 入口和会话选择：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/cmd/run.go:62-156, 667-729; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/ui/model/ui.go:4083-4142。
- HTTP 接受和分离分派：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/server/proto.go:763-799; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/backend/agent.go:15-121。
- SSE 订阅/事件解码：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/server/proto.go:270-334; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/client/proto.go:112-264。
- Coordinator 设置、MCP 就绪、模型/提供商选项、认证重试和 RunComplete 合并：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/coordinator.go:155-337。
- 分派、队列、取消标记、活动上下文注册、提示/上下文组装：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/agent.go:355-498, 565-782, 1509-1707, 1954-2020。
- Fantasy 流回调、工具调用/结果持久化、完成处理、循环/摘要终止、错误恢复、队列递归：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/agent.go:795-1063, 1066-1326。
- 工具构造和过滤：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/coordinator.go:623-801。
- 钩子和批准策略：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/hooked_tool.go:15-99; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/permission/permission.go:112-277; bash 执行策略 /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/agent/tools/bash.go:196-384。
- 持久化和恢复：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/session/session.go:64-107, 170-220; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/message/message.go:104-119, 163-286, 457-487。
- 最终输出及关联/协调：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/cmd/run.go:318-454。
- UI 事件渲染和权限/问题对话框：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crush/internal/ui/model/ui.go:695-976。
