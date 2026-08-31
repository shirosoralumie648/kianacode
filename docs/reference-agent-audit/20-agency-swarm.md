# agency-swarm — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/agency-swarm`
- **主要运行时：** agency-swarm
- **审计范围：** `src agency, agents, tools, communication, runs and tests`

本报告基于源文件、测试、清单以及可执行入口。

## 端到端流程

- 1. FastAPI `/get_response` 或 `/get_response_stream` 接收消息及可选的扁平 `chat_history`；工厂通过 load callback 创建全新的 Agency。
- 2. Agency 解析默认或选定的入口 agent，挂接持久化 MCP servers，合并 context 与 hooks，并可选地加入 recipient-switch reminder。
- 3. Agent 创建 run ID/trace ID，处理文件，组合 shared/base/additional instructions，加载相关扁平历史，持久化新的 user item，移除 framework metadata，并构建 `MasterContext`。
- 4. Agency 调用 SDK 的 `Runner.run` 或 `Runner.run_streamed`；SDK 执行 provider 请求、流解析及其循环。
- 5. 模型产生 FunctionTool call 时由 SDK 调用。`SendMessage` 解析/校验 JSON，阻止同一接收者的重复进行中调用，调用子 agent，并将子 agent 文本作为 tool output 返回给 SDK 继续执行。Handoff 切换 SDK agent 并追加 reminder。
- 6. Guardrail 触发器反馈导致有界重试或 guidance/error 结果。SDK 最终输出或中断结束循环；Agency 不负责 session resume。
- 7. Run items 变为经过清理、补充 metadata 的持久化消息；流式 item 在最终持久化前会 reconciliation、去重并过滤孤儿项。
- 8. 直接调用方收到 `RunResult.final_output`；SSE 调用方收到序列化事件以及最终 `messages`/usage/end；AG-UI 调用方收到适配后的 AG-UI 生命周期、消息和错误事件。
- 9. Stream cancellation 从 endpoint registry/disconnect 传播到 stream wrapper 与 SDK streaming result；随后 cleanup 持久化仍存的 message prefix/tail 并注销 run。

## 组件与边界

- 主要运行时是基于 `openai-agents==0.18.1` 的 Python 3.12+ `agency_swarm`；包暴露 `agency-swarm` CLI，但请求路径由 library/SDK 驱动。清单：`reference/agency-swarm/pyproject.toml:7-39,58-59`。次级运行时/路径为 FastAPI/SSE 和 AG-UI（`src/agency_swarm/integrations/fastapi.py`、`integrations/fastapi_utils/endpoint_handlers.py`）、realtime（`src/agency_swarm/integrations/realtime*.py`、`src/agency_swarm/realtime/*`）以及 CLI/TUI（`src/agency_swarm/cli/*`、`src/agency_swarm/ui/*`）。
- 代表性请求：客户端向默认入口 agent 发送“Find the latest revenue and ask the Analyst agent to summarize it.”。相同路径适用于任意用户消息；模型决定直接回答、调用本地/托管/MCP 工具，或触发 framework communication。
- Ingress 与 agency 设置：`Agency.__init__` 校验入口点/flow，创建 `ThreadManager` 和可选的 `PersistenceHooks`，注册/配置 agents，初始化每个 agent 的 runtime state，并安排可选 starter-cache warmup（`src/agency_swarm/agency/core.py:108-253`）。`Agency.get_response` 解析 recipient 并委托 `agency.responses.get_response`（`core.py:357-409`；`agency/responses.py:213-250`）。流式路径同样解析 recipient，但创建 `EventStreamMerger` context，并通过 `context_override` 传递（`agency/responses.py:350-408`）。
- Thread/session：主要 Agency runtime 不创建 OpenAI SDK `Session`、`conversation_id` 或 durable run-state，而是通过 `ThreadManager` 建立进程内扁平 `MessageStore`；构造时调用可选 load callback（`src/agency_swarm/utils/thread.py:121-149,208-223`）。FastAPI 每个请求创建新 Agency，并将 `request.chat_history` 加载到该 store（`src/agency_swarm/integrations/fastapi_utils/endpoint_handlers.py:1068-1103,1203-1239`）。每次 append 调用 save callback，流式 cleanup 期间使用显式 replace/persist（`utils/thread.py:151-175,224-243`）。因此 resume 是 callback/history replay，而不是 SDK session；AG-UI `thread_id` 只在事件中发出/关联，不是 persistence key（`endpoint_handlers.py:1650-1669,1800-1808`）。
- Prompt/context：`Agent.get_response` 惰性确保 MCP tools，并调用 `Execution.get_response`（`src/agency_swarm/agent/core.py:685-736`）。Execution 临时组合 shared 与 per-run instructions，动态对齐 runtime handoffs（`agent/execution_helpers.py:305-414`），处理附件，生成 `agent_run_id`/trace ID，并调用 `MessageFormatter.prepare_history_for_runner`（`agent/execution.py:103-145`）。formatter 选择相关 user 或 agent pair history，加入 agency metadata，持久化 initiating message，清理 tool-call history/content/IDs，移除 framework metadata，返回 SDK input（`messages/message_formatter.py:257-320,369-447`）。`MasterContext` 合并 agency user context 与 override，并指向共享 runtime-state map（`agent/execution_helpers.py:231-272`）。
- Model/loop：非流式 `run_with_guardrails` 调用 `Runner.run`，准备 input/context、run config 和默认 `max_turns=1,000,000`（`agent/execution_helpers.py:56-89,95-186`；`agent/execution.py:58-59,212-227`）。流式调用 `Runner.run_streamed` 并手动 drain `stream_events()`（`agent/execution_streaming.py:124-143,234-311`）。SDK Runner 负责 model dispatch、响应解析、tool-call 选择、tool execution、result 注入和 continuation；Agency Swarm 只是包装该循环。run 会在 SDK 返回最终输出、配置的 tool-stop、handoff 完成、中断、错误或达到 max turns 时结束。Agent 默认值暴露 SDK tool behavior（`agent/core.py:171-196`）。
- Tool-call：framework-visible calls 作为 SDK `RunItem`/`ToolCallItem` 到达，并用 `RunItem.to_input_item()` 转为 history（`agent/execution_helpers.py:193-205`）。生成的 `SendMessage` FunctionTool 解析 JSON，校验 recipient 和额外 Pydantic 参数，阻止同一 recipient/thread 的并发发送，然后同步调用 recipient 的 `get_response` 或 `get_response_stream`（`tools/send_message.py:313-427`）。子 agent 最终输出作为 tool result 返回（`send_message.py:474-538`），SDK 将其注入 parent loop 后要求 parent model 继续。流式事件经共享 streaming context 转发（`send_message.py:429-497`；`agency/responses.py:372-408`）。Handoffs 是 SDK handoff objects，并把 recipient reminder 持久化进 history（`tools/send_message.py:569-641`）。Local BaseTool 的 schema/execution 由 SDK 提供；BaseTool 暴露 schema、context 和抽象 `run`（`tools/base_tool.py:72-171`）。
- Policy/approval：主要路径没有独立 Agency-level authorization engine。SDK FunctionTool 支持 `is_enabled`、`needs_approval`、input/output guardrails 与 timeout，由 compatibility wrapper 暴露（`tools/function_tool_compat.py:159-214`）；实际审批与执行委托给 `openai-agents`。MCP approval request/response item 依据关联 ID 被历史过滤保留或移除（`messages/message_filter.py:45-55,102-155,199-213`），realtime 则发出 `tool_approval_required` 事件（`integrations/realtime_events.py:122`）。FastAPI `RequestOverridePolicy` 是 provider/model/client override policy，不是 tool authorization（`integrations/fastapi_utils/override_policy.py:18-85`）。
- 持久化与输出：非流式转换每个 SDK new item，添加 agent/caller/run/trace metadata，包含 hosted search 结果/citations，过滤/规范化 ID 并追加到扁平 store（`agent/execution.py:283-346`），返回 `RunResult.final_output`（`execution.py:274-288,373-388`）。流式在事件到达时持久化，并以 hybrid identity/message-ID/call-ID/content-hash 匹配再次 reconciliation，删除重复/孤儿项，持久化 sanitized tail（`agent/execution_stream_persistence.py:158-253,256-291,302-487`）。stream wrapper 在正常结束时解析最终 SDK result（`agent/execution_streaming.py:482-513`）。FastAPI 非流式返回 `response.final_output` 与新消息（`endpoint_handlers.py:1156-1182`）；SSE 发出序列化事件、最终 `messages`、usage 及 `end/[DONE]`（`endpoint_handlers.py:1464-1543`）；AG-UI 转换 OpenAI/SDK 事件并发出 RUN_STARTED/RUN_FINISHED 或 RUN_ERROR（`endpoint_handlers.py:1800-1985`）。
- Guardrails/retry/恢复：output guardrail 触发会追加反馈并在 `validation_attempts` 内重试；input guardrail 按 `raise_input_guardrail_error` 返回 guidance 或抛错；其他 Runner 错误包装为 `AgentsException`（`agent/execution_helpers.py:95-190`）。流式维持 retry loop，并在持久化前裁剪 guardrail 创建的分支（`agent/execution_streaming.py:169-180,449-531,600-676`）。取消仅实现于 streamed runs：`StreamingRunResponse.cancel` 通知 worker，worker 调用 SDK `RunResultStreaming.cancel(mode=...)`，支持立即/回合后 drain，并在超时后强制取消（`agent/execution_streaming.py:245-311,532-590`；FastAPI registry/cancel endpoint `endpoint_handlers.py:668-710,1584-1630`）。客户端断开也立即取消（`endpoint_handlers.py:1472-1479`）。Persistence callback 的 load/save 失败会记录日志并吞掉，因此 run 可能以空/过期历史继续（`utils/thread.py:208-243`）。
- 测试覆盖扁平 thread storage、callback persistence、streaming wrapper/final-result synchronization、取消与 registry cleanup、handoff/context/parent-run、communication streaming、guardrail、tools、MCP 及 AG-UI/FastAPI，具体测试路径见源文件中的测试套件。

## 状态、持久化与恢复

- 仅作只读审计；未创建、修改、删除、移动或复制文件。
- 未使用 README.md、CLAUDE.md、AGENTS.md、USER.md、docs、changelog 和 git history。
- 检查了源文件、测试、pyproject manifest 及 executable/runtime entrypoint。
- 未执行运行时或外部模型请求；结论是静态源码追踪。

## 工具、策略与副作用

- Read：源码、manifest 与测试检查。
- Bash：对允许的源码和测试路径进行只读 find/grep 索引。
- 未使用 write/edit/delete 或状态改变命令。

## 输出与呈现

- 审计状态：已完成主要 Python/openai-agents runtime 的 source-only trace。
- 最强架构边界是：provider response parsing、native tool-call execution、tool-result injection 和 SDK loop termination 位于 `openai-agents`；Agency Swarm 用 multi-agent context、SendMessage/handoff wiring、guardrails、persistence、event decoration 及 HTTP/UI adapters 包装它们。
- 持久对话原语是通过 callback 加载的扁平消息列表；它适合 request-provided history 与 callback persistence，但不等价于 SDK Session/RunState recovery，也不会按 AG-UI thread ID 存储。
- Approval 通过 SDK FunctionTool/MCP/realtime 接口暴露，而非由单一 Agency Swarm policy layer 强制；部署特定的审批行为必须在 SDK/provider integration 与 realtime/MCP 路径核验。

## 测试与验证

- 未执行测试；这是按要求进行的 source-code-only audit。
- 相关静态测试覆盖见 `tests/test_agent_modules/test_thread_manager.py:225-297`、`tests/integration/persistence/test_persistence.py:144-205,267-300`、`tests/test_agent_modules/test_execution_streaming.py:18-133`、`tests/integration/fastapi/test_fastapi_stream_cancellation.py:52-158` 及证据中列出的 communication/agency/guardrail/MCP/FastAPI suites。

## 优势

- Agency orchestration、Agent execution、message formatting、streaming persistence 与 FastAPI/UI adapters 分离清晰。
- 强健的 run/trace/parent IDs 与 metadata propagation 使 multi-agent history 和 streamed handoff 可观测。
- Streaming cancellation 协作式执行，支持 immediate/after-turn、drain SDK events 并清理 active-run registry。
- Streaming persistence 对 recreated IDs、call IDs、fake IDs 和 content hashes 具有稳健 hybrid matching。
- Guardrail 处理包含有界 output retries 和明确的 branch pruning tests。
- 全面的测试覆盖 persistence、delegation、streaming final results、cancellation、guardrails、MCP、tools、AG-UI 和 FastAPI。

## 风险与缺口

- 主要执行行为部分位于仓库外的 `openai-agents==0.18.1`；仅源码检查不能证明 provider-specific tool parsing、approval resolution 或 model API semantics，超出 wrapper call sites 的部分尤其如此。
- Persistence callback 异常被吞掉（`utils/thread.py:208-243`），可能在 load/save 失败后产生看似成功的响应并丢失恢复状态。
- 新建 FastAPI Agency 与请求提供的 `chat_history` 使 history ownership 成为应用责任；callback 未正确隔离时，用户/聊天可能混用或历史丢失。
- 非流式没有对应的 active-run cancellation endpoint；取消只围绕 streaming 实现。
- 默认 `max_turns=1000000` 实际近乎无限，除非调用方提供上限（`agent/execution.py:58-59`、`execution_helpers.py:86-88`、`execution_streaming.py:140-142`）。

## 映射到 Kiana

- Ingress/session：Agency 加 FastAPI endpoint factory 与 callback-loaded flat ThreadManager。
- Context/prompt：MessageFormatter、setup_execution、MasterContext、runtime state、shared/additional instructions。
- Model/loop：openai-agents Runner.run/run_streamed；Agency Swarm 提供 guardrail 与 persistence wrappers。
- Tools/policy：SDK FunctionTool/handoff/MCP integration；SendMessage 是 framework 的具体 communication tool；没有独立中央 approval engine。
- Persistence/recovery：callback-backed flat message list、per-message agency metadata、guardrail pruning 与 stream reconciliation；Agency 路径没有 first-class SDK session。
- Events/UI：StreamingRunResponse、EventStreamMerger、SSE endpoint、AG-UI adapter、realtime event bridge。
- Tests：证据中列出的 unit、integration、FastAPI、persistence、communication、guardrail、MCP 与 tool suites。

## 源码证据

- `reference/agency-swarm/src/agency_swarm/agency/core.py:108-253` 展示初始化、runtime state、persistence hooks 与 entry-point registration。
- `reference/agency-swarm/src/agency_swarm/agency/responses.py:213-250,350-425` 展示 direct/streaming ingress、recipient resolution、context、MCP attachment 与 merger wiring。
- `reference/agency-swarm/src/agency_swarm/agent/execution.py:103-145,212-346,390-405` 展示 ID/context/history assembly、SDK invocation、item-to-history conversion 与 cleanup。
- `reference/agency-swarm/src/agency_swarm/agent/execution_helpers.py:56-89,95-190,231-272,305-414` 展示 Runner calls、guardrails、errors、context construction 与 temporary instruction/handoff wiring。
- `reference/agency-swarm/src/agency_swarm/agent/execution_streaming.py:169-311,371-447,449-565` 展示 streaming worker、cancellation、event forwarding、persistence、guardrail handling 与 cleanup。
- `reference/agency-swarm/src/agency_swarm/tools/send_message.py:313-538` 展示 JSON tool-call parsing、recipient policy/concurrency guard、subagent execution、event forwarding 与 result return。
- `reference/agency-swarm/src/agency_swarm/utils/thread.py:121-243` 展示 callback-backed flat history load/save 与 failure behavior。
- `reference/agency-swarm/src/agency_swarm/integrations/fastapi_utils/endpoint_handlers.py:1068-1199,1203-1630,1633-1987` 展示 HTTP ingress、fresh-agency history loading、SSE/AG-UI output、disconnect/cancel 与 final payloads。
