# autogen — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/autogen`
- **主要运行时：** autogen
- **审计范围：** `python, dotnet, protocol, agents, teams, tools and runtime`

本报告基于源文件、测试、清单文件和可执行入口。

## 端到端流程

- 使用的具体请求：`RoundRobinGroupChat.run_stream(task='Write a program...')`，其中 AssistantAgent 配置了 FunctionTool 和 ReplayChatCompletionClient。Team 创建用户 TextMessage，启动嵌入式运行时，manager 发布任务，轮询选择 assistant，container 将其单条消息缓冲区传给 AssistantAgent。
- AssistantAgent 添加任务和从 memory 得到的上下文，组装 system/context/tool schema，调用 ReplayChatCompletionClient.create/create_stream，接收 FunctionCall，发出 ToolCallRequestEvent，解析 JSON 参数，调用 StaticStreamWorkbench/FunctionTool，发出 ToolCallExecutionEvent 并注入 FunctionExecutionResultMessage。
- 当 reflect_on_tool_use 为 false 时，工具结果以 ToolCallSummaryMessage 返回；container 发布它，manager 将其追加并选择下一位 speaker。当 max_tool_iterations > 1 时，会改为再次请求模型；启用反思后，会发出 `tool_choice='none'` 请求并返回文本/结构化输出。
- Manager 在每个完成的响应或 max_turns 后应用终止条件，发布 GroupChatTermination，team 队列消费者产出 TaskResult，嵌入式运行时的 stop_when_idle 传播后台异常。Console 可以消费相同的流并呈现块/事件/最终输出。
- 后续的 `run(task=None)` 会继续使用持久化于内存中的 participant/model 上下文和 manager 轮询索引；save_state/load_state 支持转移到新的 team 实例。取消会通过 runtime、模型调用、工具调用、队列读取和用户输入串联 futures；失败会为 group 输出序列化并重新抛出。

## 组件与边界

- 主要 Python 运行时：基于 autogen-core 0.7.5 的 autogen-agentchat 0.7.5；AssistantAgent、ChatAgentContainer、BaseGroupChat/Manager、SingleThreadedAgentRuntime、Workbench/FunctionTool、模型客户端和 Console 构成可执行路径。
- 次级 .NET 运行时：现代 Microsoft.AutoGen.AgentChat group-chat/runtime 栈，以及旧版 AutoGen ConversableAgent middleware 栈。Python 是主要实现，因为其包清单和测试定义了当前最完整的 AgentChat 实现。

## 状态、持久化与恢复

- 已完成仅基于源码的审计；未修改文件。
- 已完成对 Python agent、team、工具、运行时、协议、持久化、取消、UI 和测试的主要审查。
- 已识别次级 .NET 路径，并抽样检查关键入口/agent/group/state 源码。
- README.md、CLAUDE.md、AGENTS.md、USER.md、docs 目录、变更日志和 git 历史均未打开。

## 工具、策略与副作用

- 通过 Bash/Read 使用 find、grep 和只读源码检查
- 结构化的源码到测试跟踪
- 未使用写入/编辑/删除命令

## 输出与呈现

- Python 运行时通过明确的行引用和测试，跟踪从完整用户请求到答案的路径。
- 工具调用被解析为模型 FunctionCall 对象并并发执行；工具结果既作为事件可观察，也注入后续模型上下文。
- 终止、循环、取消、恢复、流式传输和 UI 输出均已实现，并由源码测试覆盖。
- .NET 实现存在实质差异：旧版 ConversableAgent 使用有序 middleware 处理系统提示、人工输入、函数调用和内部模型；现代 .NET AgentChat 使用 runtime/topic group 架构。

## 测试与验证

- 未执行测试；审计仅依赖阅读源码和测试定义。
- 检查了 test_assistant_agent.py、test_group_chat.py、test_userproxy_agent.py 中相关的可执行测试定义，并枚举了 .NET AgentChat/Grpc 测试路径。

## 优势

- AgentChat 编排与 Core 运行时/工具/模型抽象之间分层清晰。
- 流式传输从模型块贯穿工具事件、team 输出队列直到 Console，是一等能力。
- 工具调用循环具有明确的迭代上限、handoff 处理、反思模式、结构化输出和错误作为结果的行为。
- agent、container、manager、team 和 runtime 层均存在状态 API，并由往返测试覆盖。
- 运行时取消和干预 hooks 提供有用的传输层控制，而序列化异常保留失败诊断信息。

## 风险与缺口

- Python AssistantAgent 在模型工具调用解析与执行之间没有内置人工审批/策略决策；部署必须增加外部机制或自定义 Workbench/intervention 架构。
- 源码文档说明 SingleThreadedAgentRuntime 面向开发/独立运行，而非高吞吐；它为每条排队消息运行一个任务，后台异常行为取决于 ignore_unhandled_exceptions（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-core/src/autogen_core/_single_threaded_agent_runtime.py:148-164）。
- 源码有意说明取消可能使 team 处于不一致状态；取消可以中断队列/模型/工具/用户 futures，而状态只有通过 save_state 才会显式持久化。
- AssistantAgent 和模型上下文不支持并发调用的 coroutine 安全，因此共享 agent 必须在外部串行化（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/agents/_assistant_agent.py:108-119）。
- FunctionTool 配置加载会执行导入和源代码；源码明确警告只能加载受信任的配置（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-core/src/autogen_core/tools/_function_tool.py:142-180）。
- Team 持久化以状态为中心，SingleThreadedAgentRuntime 不会持久化 runtime 订阅状态；运行时源码明确说明订阅不会保存/加载（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-core/src/autogen_core/_single_threaded_agent_runtime.py:430-463）。

## 映射到 Kiana

- 入口：BaseGroupChat.run_stream -> GroupChatStart
- 编排：BaseGroupChatManager thread + speaker transition
- Agent 执行：ChatAgentContainer -> AssistantAgent.on_messages_stream
- 上下文/模型：AssistantAgent context/memory/_call_llm
- 工具：Workbench -> FunctionTool -> FunctionExecutionResultMessage
- 控制：termination conditions/max_turns/handoff/reset/pause/resume
- 持久性：save_state/load_state 用于 agent 上下文、缓冲区和 manager thread
- 呈现：output queue -> Console 或 TaskResult
- 传输：SingleThreadedAgentRuntime；分布式 .NET/core 路径使用 protobuf AgentRpc/CloudEvent contracts

## 源码证据

- 入口/team 启动：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_base_group_chat.py:246-577` 将字符串任务转换为 TextMessage(source='user')，验证注册和运行状态，初始化/注册 runtime agent 和订阅，发送 GroupChatStart，排空输出队列，产出 TaskResult，并执行关闭/队列清理。
- 路由/线程：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_base_group_chat_manager.py:85-131` 验证并发布初始消息，更新 `_message_thread` 并选择 speaker；`:134-169` 将内部及最终响应折叠到 thread，应用终止并安排下一位 speaker；`:171-227` 强制 speaker 有效性、最大轮数和终止条件重置/信号。
- Agent container：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_chat_agent_container.py:55-158` 缓冲消息，调用 delegate stream，将内部事件转发到输出，并发布 GroupChatAgentResponse 或 GroupChatError。具体的轮询 speaker/index 和 manager 状态在 `_round_robin_group_chat.py` 的 `:47-81` 和 `:57-69`。
- 提示/模型路径：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/agents/_assistant_agent.py:900-1010` 添加传入/Handoff 上下文，应用 memory，调用模型，产出块/思考，追加 AssistantMessage，然后处理结果。`:1054-1115` 组装 system 加 context，列出 workbench 和 handoff 工具，并使用取消和结构化输出选项调用 create/create_stream。
- 工具调用循环/结果注入：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/agents/_assistant_agent.py:1117-1325` 发出请求事件，并发执行 FunctionCalls，转发流式工具事件，将 FunctionExecutionResultMessage 追加到模型上下文，发出执行事件，处理 handoff，循环最多执行 max_tool_iterations 次，然后反思或汇总。解析/执行/错误行为位于 `:1535-1624`；无效 JSON 会变成错误 FunctionExecutionResult，缺失工具会变成错误结果。
- 工具实现：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-core/src/autogen_core/tools/_base.py:175-207` 通过 run_json 验证类型化参数，并跟踪/记录调用；`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-core/src/autogen_core/tools/_static_workbench.py:67-123` 和 `:177-225` 列出工具、分派调用、捕获异常，并暴露 ToolResult 错误状态。
- 策略/审批：AssistantAgent 在执行模型选择的工具之前没有专用审批门。唯一通用的拦截点是 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-core/src/autogen_core/_single_threaded_agent_runtime.py:670-790` 的 runtime InterventionHandler hooks，用于 send/publish/response；AssistantAgent 的工具执行绕过这些 hooks，直接调用 Workbench。
- 持久化/恢复：AssistantAgent 在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/agents/_assistant_agent.py:1626-1639` 保存/加载模型上下文。Team 在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_base_group_chat.py:747-834` 保存按名称索引的 participant/container 和 manager 状态，并通过 runtime 恢复；container 状态包含其缓冲消息，位于 `:196-213`。`run(task=None)` 恢复缓冲/thread 状态，reset 在 `_base_group_chat.py:579-655` 向 agent 发送 reset RPC。
- 输出/UI：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/ui/_console.py:81-203` 呈现事件，合并流式块，打印最终 Response/TaskResult，并在不呈现 UserProxy 输入的情况下发出信号。用户输入/取消实现在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/agents/_user_proxy_agent.py:203-236`。
- runtime/协议：排队的 direct/publish envelope 和取消在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-core/src/autogen_core/_single_threaded_agent_runtime.py:331-428` 和 `:465-628` 定义/处理。JSON/Pydantic/dataclass/Protobuf 序列化位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-core/src/autogen_core/_serialization.py:101-183` 和 `:224-257`。跨进程 RPC/state/payload contract 位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/protos/agent_worker.proto:10-35` 和 `:81-133`。
- 测试：工具循环、最大迭代、handoff、持久化、无效 JSON 和 memory 测试位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/tests/test_assistant_agent.py:527-675`、`:1213-1359` 和 `:1371-1437`。端到端 group 工具/流/Console 行为位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/tests/test_group_chat.py:568-631`；恢复/reset、异常、最大轮数和取消位于 `:635-759`。用户输入/handoff/取消/错误测试位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/python/packages/autogen-agentchat/tests/test_userproxy_agent.py:10-119`。
- 次级 .NET 证据：现代 group 入口/runtime/流/输出/状态生命周期位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/dotnet/src/Microsoft.AutoGen/AgentChat/GroupChat/GroupChatBase.cs:72-347`；旧版 agent 入口、系统提示插入、middleware 顺序、人工输入策略、函数 middleware 和模型委托位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/autogen/dotnet/src/AutoGen/Agent/ConversableAgent.cs:30-187`。
