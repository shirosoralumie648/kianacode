# agent-framework — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/agent-framework`
- **主要运行时：** agent-framework
- **审计范围：** `python, dotnet, go agent abstractions and execution flows`

本报告基于源文件、测试、清单文件和可执行入口。

## 端到端流程

- 入口：AG-UI FastAPI POST 接收 thread/run 请求，应用调用方提供的授权范围和默认值，然后打开 SSE 响应。
- 线程/会话：`thread_id` 和 `run_id` 由调用方提供或生成；有范围限定的 `ThreadSnapshotSession` 加载最新可重放的 transcript/state/interrupt/private session continuation。新运行时，`AgentSession` 使用 AG-UI thread ID 作为本地会话 ID，并可选使用所提供的 provider thread 作为服务会话 ID。
- 提示/上下文：AG-UI 规范化快照和传入消息；核心 `RawAgent` 规范化消息，合并默认/运行时选项，解析本地历史与服务历史，适时自动添加内存历史，执行上下文提供程序，然后在模型输入前追加 provider 消息/指令/工具。
- 模型请求/流：`RawAgent` 使用组装好的消息/选项/工具/客户端 kwargs 调用 `SupportsChatGetResponse`。客户端/提供商实现负责 HTTP/提供商转换；`ResponseStream` 更新会被映射并最终化为 `AgentResponse`。
- 工具调用解析/策略：模型的 `function_call` 内容会被解析并按 schema 验证。function 层将调用分类为未知、仅声明、需审批、托管和可执行调用；审批请求会持久化在会话/生命周期状态中，而不会执行。
- 工具执行/结果注入：可执行调用通过 function middleware 和 `FunctionTool.invoke` 运行；并行结果变为 `function_result` 内容。结果被追加到 role=tool 消息中，下一次模型请求会收到更新后的 transcript。
- 循环/终止：每批工具都会被处理，错误计数、continuation ID 更新，循环默认最多执行 40 次模型迭代。审批/声明/用户输入请求会返回暂停；middleware 终止或错误上限会停止；循环耗尽时强制执行一次禁用工具的最终响应。
- 持久化/恢复：提供商 after-run 保存受 egress 策略延迟控制。AgentSession 和历史存储支持内存副本、原子文件快照/历史、状态 codec、版本检查、路径验证和损坏隔离。AG-UI 存储可重放快照和私有 continuation 状态；恢复会重建消息并验证每个待处理的 interrupt。事件/UI：AG-UI 通过 SSE 发出 RunStarted、状态快照、文本增量、工具开始/参数/结束/结果、审批中断、消息快照和 RunFinished。.NET 通过 IAsyncEnumerable 暴露 `AgentResponseUpdate`，并依赖宿主消费者进行呈现。
- 取消/错误：取消令牌贯穿 .NET 模型/提供商调用；Python hooks 处理 CancelledError，工具循环取消会取消同批的兄弟任务。AG-UI 将端点流异常捕获为通用 RunError 事件；未完成的运行会恢复审批所有权，但进程本地的审批/快照存储无法跨重启保留。

## 组件与边界

- 主要运行时：Python 包 `python/packages/core/agent_framework` 及 AG-UI 宿主（`python/packages/ag-ui`）。推荐的公共 agent 是 `Agent`；`RawAgent` 提供核心模型接缝。次级运行时：.NET `dotnet/src/Microsoft.Agents.AI.Abstractions` 和 `dotnet/src/Microsoft.Agents.AI`；`go` 下没有 Go 源码（只有被排除的 README.md）。
- Python 核心：`RawAgent.run` -> `_prepare_run_context` -> `_prepare_session_and_messages` -> chat client -> response parser/provider persistence。`AgentExecutor` 添加工作流串联、流式输出、request_info 审批暂停和检查点状态。
- Python 协议/UI：FastAPI AG-UI POST 入口 -> `run_agent_stream` -> 快照/恢复/审批重建 -> 流式 agent 更新 -> AG-UI 文本/工具/状态事件 -> 快照和 RUN_FINISHED。
- 次级 .NET：`AIAgent` 定义运行/会话/序列化抽象；`ChatClientAgent` 准备提供商/历史并调用 `IChatClient`；工具审批由 `ToolApprovalAgent` 和 `ApprovalResponseBindingChatClient` 实现。
- Go：没有找到可执行/源码抽象；只有 `go/README.md`，按请求未打开。

## 状态、持久化与恢复

- 仅进行了源码观察；未作修改。
- README.md、CLAUDE.md、AGENTS.md、USER.md、docs 目录、变更日志和 git 历史均未打开。
- 运行时清单发现 Python 和 .NET 源码；Go 目录除被排除的 README.md 外没有源文件。
- 证据中的精确行范围在可行时使用仓库相对范围；本报告中的所有路径均按工作器指令保持绝对路径。

## 工具、策略与副作用

- Read、Bash（仅 find/grep/列举）、并行工具调用、StructuredOutput。未尝试任何写入/编辑/删除操作。

## 输出与呈现

- 一个具体的天气请求完整经过 FastAPI POST、AG-UI 线程/会话恢复、上下文和提示组装、流式模型响应、函数调用策略、工具执行/结果注入、有界循环、事件编码以及快照/RUN_FINISHED 持久化。
- Python 是主要实现，因为它包含端到端 HTTP/AG-UI 入口以及最丰富的源码/测试覆盖；.NET 是实质性的次级实现，具有类似抽象和一致性测试。
- 该实现以可恢复性和治理为设计目标，但默认审批/快照存储是进程本地的，且通用端点错误会丢失诊断细节。

## 测试与验证

- Python AG-UI：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/tests/ag_ui/test_multi_turn.py:48-145` 基本/工具 transcript 往返；`:151-262` 审批中断/恢复；`:270+` 工作流中断/恢复。
- Python 工作流：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/core/tests/workflow/test_agent_executor_tool_calls.py:115-184,266-400,503-600`。
- Python 审批生命周期：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/tests/ag_ui/test_approval_lifecycle.py:49-86,150-234,274-347,719-854,889-1045`。
- .NET 一致性：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/dotnet/tests/AgentConformance.IntegrationTests/RunTests.cs:21-121`；流式/工具 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/dotnet/tests/AgentConformance.IntegrationTests/ChatClientAgentRunStreamingTests.cs:19-69`。

## 优势

- 入口协议适配器、agent 抽象、上下文提供程序、模型客户端、工具调用和持久化之间分离清晰。
- transcript/会话处理能力强：有范围的线程快照、显式会话 codec、原子文件替换、损坏隔离、去重和 continuation ID 跟踪。
- 审批流程默认拒绝且支持重放：请求受范围约束、绑定到模型发起的调用、由生命周期跟踪、可恢复，并针对重复执行/取消/冲突进行测试。
- 工具循环具备 schema 验证、未知/仅声明处理、错误转换、可配置的迭代/错误/调用预算、并行执行以及明确的终端回退。
- 流式传输在延迟的消费者拉取期间保留提供商会话 ID 和运行身份；.NET 会显式释放流，并在失败时通知提供商。
- 测试覆盖普通运行、多轮重放、流式传输、工具调用、审批、仅声明工具、工作流恢复、生命周期保留、容量和幂等性。

## 风险与缺口

- AG-UI `InMemoryAGUIApprovalStateStore` 是进程本地且不持久化的（`_approval_state.py:37-44`），因此重启或副本路由可能丢失待处理审批权，除非提供持久化的外部存储。
- AG-UI 默认内存快照是进程本地且有界的（`_snapshots.py:120-206`）；thread ID 不是授权边界，端点设置正确地要求显式 scope resolver。
- AG-UI 端点将所有流/编码异常转换为通用 `RunErrorEvent`/HTTP 500（`_endpoint.py:222-278`），除非能够访问服务器日志，否则会降低客户端诊断能力。
- Python 兄弟任务取消无法停止已经在 `asyncio.to_thread` 中运行的同步工具体（`_tools.py:1863-1875`），因此即使结果被丢弃，副作用也可能在批次取消后完成。
- 当存在 metadata 时，AG-UI 强制设置 `options={metadata:safe_metadata, store:True}`（`_agent_run.py:2513-2521`）；必须有意配置并审查提供商侧持久化行为，以处理敏感 transcript 保留问题。
- 会话快照/历史是明文 JSON/msgpack/JSONL，除非应用保护存储；源码注释和 FileSessionStore 代码将加密/访问控制作为应用责任。
- .NET 说明消息和序列化会话是不受信任/敏感的；调用方必须验证提示/输出并保护会话存储。

## 映射到 Kiana

- Python 主要实现：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/core/agent_framework/_agents.py` 和 `_tools.py`。
- Python 入口/UI：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/agent_framework_ag_ui/_endpoint.py`、`_agent.py`、`_agent_run.py`、`_snapshots.py`、`_approval_state.py`。
- Python 测试：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/tests/ag_ui/test_multi_turn.py`、`test_approval_lifecycle.py`、`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/core/tests/workflow/test_agent_executor_tool_calls.py`。
- 次级 .NET：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/dotnet/src/Microsoft.Agents.AI.Abstractions/AIAgent.cs`、`.../Microsoft.Agents.AI/ChatClient/ChatClientAgent.cs`、`.../Harness/ToolApproval/ToolApprovalAgent.cs`、`.../ChatClient/ApprovalResponseBindingChatClient.cs`。
- 未找到 Go 可执行/源码路径；`go/README.md` 被有意排除。

## 源码证据

- 具体请求 `POST /`，载荷为 `{messages:[{role:user,content:'What is the weather?'}],threadId:'thread-tool-multi',runId:'run-1'}`，进入 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/agent_framework_ag_ui/_endpoint.py:167-212`；端点解析快照范围/默认状态并调用 `protocol_runner.run(input_data)`。它将每个事件编码为 SSE，并在 `:222-278` 将编码/流失败转换为通用 `RunErrorEvent`。
- `AgentFrameworkAgent.run` 在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/agent_framework_ag_ui/_agent.py:138-157` 委托给 `run_agent_stream`。`run_agent_stream` 在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/agent_framework_ag_ui/_agent_run.py:2249-2310` 选择所提供/生成的 thread/run ID，打开线程快照，协调待处理审批，加载现有快照，或重建 transcript 消息。
- 流程在 `_agent_run.py:2311-2409` 恢复状态/schema、工具和恢复条目；在 `:2475-2501` 创建 `AgentSession(session_id=thread_id, service_session_id=...)`，恢复私有 continuation 状态和审批状态；在 `:2529-2579` 调用模型前解析传入审批。
- 模型流在 `_agent_run.py:2640-2645` 通过 `(a2ui_runner or agent).run(messages, stream=True, session=session, ...)` 创建。更新在 `:2651-2759` 转换为 RunStarted、文本、工具调用、状态和审批事件；审批请求在 `:2707-2740` 注册到有范围的服务器端生命周期状态；待审批时流在 `:2759` 停止。
- 完成或暂停审批时，AG-UI 在 `_agent_run.py:2928-2976` 构建/持久化可重放消息、共享状态、中断描述符和私有会话 continuation 状态，然后发出 RUN_FINISHED。仅确认分支在 `:2591-2618` 以 `interrupt=None` 持久化。
- 核心 Python 执行从 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/core/agent_framework/_agents.py:1050-1139` 开始：写入新的运行身份，等待 `_prepare_run_context`，并选择非流式/流式客户端调用。上下文/选项/工具组装位于 `:1353-1570`；提供商在 `:1572-1651` 加载消息/工具/指令。
- 下游模型接缝是 `_agents.py:1157-1183`。非流式最终化会设置作者名称、更新服务 continuation ID、构造 `AgentResponse`，并按逆序调用提供程序（` :1218-1242`）。流式路径映射更新、传播会话 ID、在消费者拉取期间保留运行身份，并在最终化期间执行提供商持久化（`:1244-1315`）。
- 工具循环位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/core/agent_framework/_tools.py:3174-3335`（非流式）和 `:3337-3479`（流式）：解析传入审批，反复调用模型，处理工具调用，更新 continuation ID，并在 `max_iterations` 后强制进行 `tool_choice='none'` 的最终响应。默认值和错误上限定义在 `:1332-1409`。
- 工具解析/策略/执行：`_tools.py:1437-1641` 解析 JSON/模型参数，过滤内部 kwargs，直接或经由 middleware 调用，将普通异常转换为 function_result 错误，并允许显式 MiddlewareFailure/UserInputRequired 控制流逸出。`_tools.py:1725-1879` 对需审批/仅声明/未知调用进行分类并并发执行有效调用；任务取消会取消兄弟任务，但同步的 `to_thread` 体仍可能完成副作用（`:1863-1875`）。
- 审批结果在 `_tools.py:2873-2968` 被重新绑定/重放，并转换为终端工具内容；未解决请求会导致返回/request-info，错误可以停止循环，成功结果会作为 role `tool` 消息注入。AgentExecutor 在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/core/agent_framework/_workflows/_agent_executor.py:290-316` 暴露工作流 request_info，并在所有响应到达时恢复；它在 `:372-455` 和 `:457-545` 发出完整或增量输出。
- 持久化/恢复：`AgentSession.to_dict/from_dict` 位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/core/agent_framework/_sessions.py:1717-1791`；SessionStore 复制语义在 `:1795-1868`；FileSessionStore 在 `:1872-2038` 使用类型化 msgspec 快照、原子 `os.replace`、损坏快照隔离以及版本/路径验证。InMemoryHistoryProvider 在 `:2087-2168` 加载/存储去重消息；FileHistoryProvider 在 `:2171-2455` 读取/写入加锁的 JSONL/msgpack 历史。
- 持久化受 egress 控制：`_sessions.py:1077-1195` 将提供商持久化延后到策略判定之后；嵌套工具调用会在 `_tools.py:1675-1695` 暂停该控制。AG-UI 快照存储明确按 scope+thread 建键，但默认是有界内存存储，位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/agent_framework_ag_ui/_snapshots.py:120-206`；配置时端点要求 resolver，见 `_endpoint.py:53-73`。
- AG-UI 审批状态在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/agent_framework_ag_ui/_approval_state.py:37-127` 是有界、进程本地且不持久化的。生命周期测试在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/tests/ag_ui/test_approval_lifecycle.py:49-86,150-234,274-347,719-854,889-1045` 覆盖 pending/claimed/executing/settled 转换、容量、保留、幂等性、失败恢复、取消、范围隔离和托管审批。
- 重点测试覆盖 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/ag-ui/tests/ag_ui/test_multi_turn.py:48-145` 的 AG-UI 多轮快照重放和工具历史、`:151-262` 的审批中断/恢复、`:270-299+` 的工作流中断/恢复，以及 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/python/packages/core/tests/workflow/test_agent_executor_tool_calls.py:115-184,266-400,503-600` 的工作流 AgentExecutor 工具/审批/仅声明路径。
- 次级 .NET 等价入口抽象：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/dotnet/src/Microsoft.Agents.AI.Abstractions/AIAgent.cs:137-234` 创建/序列化/反序列化会话；`:333-341` 建立运行上下文并调用 RunCore；`:464-478` 使用取消令牌流式传输，并在每次 yield 后恢复上下文。`ChatClientAgent` 在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/dotnet/src/Microsoft.Agents.AI/ChatClient/ChatClientAgent.cs:202-265,704-812` 准备历史/上下文/提供程序并调用 IChatClient；流式/释放/失败/持久化位于 `:292-406`。.NET 审批队列/自动审批位于 `/media/.../dotnet/src/Microsoft.Agents.AI/Harness/ToolApproval/ToolApprovalAgent.cs:109-187,191-341`；请求绑定和伪造响应丢弃位于 `/media/.../dotnet/src/Microsoft.Agents.AI/ChatClient/ApprovalResponseBindingChatClient.cs:70-131,156-237`。
- .NET 一致性测试验证 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/dotnet/tests/AgentConformance.IntegrationTests/RunTests.cs:21-121` 的普通运行/会话历史，以及 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/agent-framework/dotnet/tests/AgentConformance.IntegrationTests/ChatClientAgentRunStreamingTests.cs:19-69` 的流式/函数调用。
