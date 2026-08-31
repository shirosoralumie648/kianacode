# crewAI — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/crewAI`
- **主要运行时：** crewAI
- **审计范围：** `lib agents, crews, tasks, tools, memory and execution`

本报告基于源文件、测试、清单以及可执行入口。

## 端到端流程

- Ingress：用户/应用调用 `Crew.kickoff(inputs)`；可选的 from_checkpoint 先恢复新实例，否则设置 environment/context/runtime scope。`stream=True` 返回 queue-backed CrewStreamingOutput，同时后台 worker 执行相同 kickoff。
- Task orchestration：sequential 或 hierarchical process 选择 tasks。`_execute_tasks` 构建前序 task context，解析 conditional/async 行为，准备 delegation/code/MCP/memory/file tools，然后执行 Task.execute_sync/async。
- Prompt/context：`Task._execute_core` 调用 task.prompt()、schema formatting、context aggregation、agent memory recall、knowledge retrieval、skill/training preparation。Executor 接收 input、tool_names、渲染后的 descriptions 和 ask_for_human_input。
- Agent loop：当前 experimental AgentExecutor 重置每次 run state 并构建 messages。启用 planning 时创建隔离的 StepExecutor contexts，只含 task description/goal 与已完成依赖结果；每个 todo 执行 LLM call -> parse -> tool -> observation iterations。PlannerObserver 负责 completion、replanning、refinement 或提前达成目标。Legacy mode 使用 CrewAgentExecutor 中共享的 ReAct/native loop。
- Model request/stream：BaseLLM 定义 call 与 stream_events wrappers、scoped call IDs 及 call start/completed/failed/chunk events。Provider implementations 格式化 messages、调用 SDK clients、支持非流式/流式。Anthropic stream 累积文本和增量 input_json tool arguments；没有 available_functions 时返回 native tool blocks。
- Tool parsing/policy/execution：文本 parser 识别 Thought/Action/Action Input/Final Answer 并修复 JSON。Native parser 接受 OpenAI function calls 与 Anthropic tool_use/name+input，并用 json.loads 解析。Before-tool hooks 可修改或阻止；approval helper 暂停 UI 并调用 input()；ToolUsage 校验并运行 BaseTool，处理 usage limits/cache/failures；after hooks 可重写 result，事件包围 execution。
- Result injection/termination：tool result 追加为 role=tool 或 Observation；post-tool reasoning prompt 驱动下一次请求。`result_as_answer` 仅在 tool result 未失败/未阻塞时可立即终止。否则在 AgentFinish、max iteration、parser/context recovery failure、timeout 或 retries exhausted 时终止。随后执行 Task guardrails、callbacks、output formatting、TaskCompleted，并创建 CrewOutput。
- Persistence/recovery：checkpoint listener 惰性订阅所有事件，将 RuntimeState entities/event record 序列化到 JsonProvider 或 SqliteProvider，并处理 lineage/pruning。from_checkpoint 检测 provider、迁移旧 payload、恢复 state、派发 restore events；crew 从第一个缺少 output 的 task 开始。Memory 使用 unified Memory；recall 是 background saves 的 read barrier，kickoff results 被提取并保存；Crew 在 kickoff completed event 前 drain 所有 memory pools。
- UI/events/cancellation：event bus 发出 crew/task/agent/LLM/tool/memory/checkpoint lifecycle events。Streaming worker 排队 frames/errors/end；耗尽前 result 不可访问。close/aclose 标记 cancelled 并关闭 iterator；generator join worker 并传播 queued exceptions。Timeout 包装 agent execution 和 future cancellation；刻意抛出的异常透传，普通失败重试整个 task。
- 测试覆盖 native tool calling、stream tool deltas、hook approval allow/block 与 UI pause/resume、checkpoint persistence/restore、flow HITL resumption/cycles 及 unified memory。

## 组件与边界

- Python package `lib/crewai/src/crewai`，包含 Crew、Task、Agent、experimental AgentExecutor、provider LLMs、tools、hooks、memory、state/checkpoints、events、streaming。
- 次级 deprecated CrewAgentExecutor：`lib/crewai/src/crewai/agents/crew_agent_executor.py`；次级 LiteAgent standalone path：`lib/crewai/src/crewai/lite_agent.py`；CLI entrypoint：`lib/cli/src/crewai_cli/cli.py`。

## 状态、持久化与恢复

- Crew 拥有 execution-scoped UUID/context 和 event runtime scope；没有通用 server session/thread object。
- Flow/conversational 次级路径接受显式 session_id 并将其映射到 flow input id。
- Agent executor 用 lock 防止一个实例并发调用，并重置每次 run 的 messages/iterations/todos。
- Memory writes 可异步，但 recall 与 Crew completion 会 drain pending saves。

## 工具、策略与副作用

- find、grep、通过只读 Bash 的 nl/sed、Read、Skill claude-api（provider/LLM-shaped audit context）、SendMessage to main agent。

## 输出与呈现

- 审计只读且仅基于源码；README、CLAUDE、AGENTS、USER、docs、changelogs 和 git history 排除。
- 代表性请求：`Crew.kickoff({request})` 进入 Crew._execute_tasks，Task._execute_core 组装 task/context/memory，Agent.execute_task 调用 AgentExecutor.invoke，planner StepExecutor 发 LLM request、解析可能的 tool call、运行 gated tool、注入 result、循环/replan，随后发出 task/crew output。
- CLI executable 为 `crewai_cli.cli:crewai`，见 `lib/cli/pyproject.toml:34-45` 与 `lib/crewai/pyproject.toml:147-155`。

## 测试与验证

- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crewAI/lib/crewai/tests/agents/test_native_tool_calling.py`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crewAI/lib/crewai/tests/agents/test_agent_executor.py`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crewAI/lib/crewai/tests/llms/test_tool_call_streaming.py`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crewAI/lib/crewai/tests/hooks/test_human_approval.py`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crewAI/lib/crewai/tests/test_checkpoint.py`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crewAI/lib/crewai/tests/test_flow_resumability_regression.py`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/crewAI/lib/crewai/tests/memory/test_unified_memory.py`

## 优势

- LLM、tool、task、agent、memory、checkpoint 和 stream lifecycle 均有清晰 event 覆盖。
- Native 与 text tool-call 兼容，支持 JSON parsing/repair 及 native 到 text mode fallback。
- Tool hook interception、usage limits、cache、guardrails、callbacks、retries 与 context-length recovery 明确。
- Checkpoint payload 含 entity state、event record、version migration、lineage、provider abstraction 和 pruning。
- Planning mode 将 step context 隔离为依赖结果，并支持 sequential/parallel ready-todo execution。

## 风险与缺口

- Agent tool execution 可扩展 policy，但默认 approval 是交互式 `input()`，无人值守执行会阻塞。
- 同步 task timeout 取消 Future，却不能强制终止 worker thread；底层 model/tool work 可能继续。
- Crew streaming cancellation 关闭 iterator 并 join background worker，因此是 cleanup/state signaling，不保证中断进行中的 provider work。
- Checkpoint event handlers 在 event-bus thread pool 执行，并有意记录/吞掉 checkpoint failures，执行可能在没有 durable checkpoint 时成功。
- Anthropic provider 用 extra_body output_format path 支持 structured outputs 和多种兼容模式，provider-specific behavior 比通用 BaseLLM contract 更复杂。
- Experimental executor 的 plan state 在普通 invoke 时重置；durable continuation 依赖 checkpoint restoration 或 Flow-specific persistence，不是隐式 chat session。

## 映射到 Kiana

- Primary runtime：Python Crew -> Task -> BaseAgent -> experimental AgentExecutor -> StepExecutor -> BaseLLM/provider -> parser/tool policy -> tool result injection -> PlannerObserver/replan -> TaskOutput/CrewOutput。
- Alternative runtime：deprecated CrewAgentExecutor 支持 classic ReAct/native loop；LiteAgent 支持带 guardrails/memory/events 的 standalone kickoff。
- 未发现其他 TypeScript、Go、Java 或 server runtime；仅有 visualization JavaScript asset。

## 源码证据

- `.../crewai/crew.py:992-1087`、`:1558-1627`；`.../crewai/task.py:806-915`；`.../crewai/agent/core.py:816-955`；`.../experimental/agent_executor.py:2802-2893`；`.../agents/step_executor.py:328-378`；`.../llms/providers/anthropic/completion.py:340-412`；`.../state/checkpoint_config.py:160-234`。
