# openai-agents-python — 源码补审

- **状态：** `audited`
- **参考路径：** `reference/openai-agents-python`
- **主要运行时：** Python `Runner` + `run_internal` 异步状态机
- **审计范围：** `Runner, model, tools, handoffs, guardrails, Session, RunState, tracing, tests`

## 总体流程

```text
Runner.run / run_streamed
  → prepare input/session/RunState
  → model response/stream
  → turn resolution
  → final output / handoff / tool execution / interruption
  → tool results + guardrails
  → next model turn
  → max-turn/error/cancel/complete
  → Session persistence + RunResult/RunState + trace/events
```

## 入口与主循环

- `reference/openai-agents-python/src/agents/run.py:248-352`：异步 `Runner.run`。
- `run.py:355-455`：`run_sync`，只是 async wrapper，不能在已有 event loop 中直接使用。
- `run.py:457-543`：`run_streamed` 返回 `RunResultStreaming`，后台 task 驱动实际运行。
- 循环语义：模型产生 final 即结束；handoff 切换 Agent；否则执行工具后再次请求模型：`run.py:268-280`。
- 非流式主实现：`run.py:543-2155`；流式：`run.py:2280-2536`；单轮编排在 `run_internal/run_loop.py`。

## Model 与流式处理

- `models/interface.py:37-135` 定义 `get_response` 和 `stream_response`；每个工具调用必须有非空、可关联 call ID，tool output 保留同一 ID。
- 非流式单轮解析动态 prompt、handoff、tools、output schema 和 input filter：`run_internal/run_loop.py:2359-2490+`。
- 流式单轮将 provider events 放入 `_event_queue`；终端 response 聚合 `ModelResponse`、usage 和 retry，再进入统一 turn resolution：`run_loop.py:2011-2356`。
- 流式生成器使用 `aclosing` 显式关闭：`:2229-2233`。
- 兼容“终端 response.output 为空但 item.done 已输出”的 provider，并累计 retry usage：`:2250-2288`。

## Result、事件队列与取消

`RunResultBase` 暴露 input、new items、raw responses、final output、guardrail results、context、last response ID 和 normalized input：`result.py:308-478`。

`RunResult` 保存 last agent、turn、max turns、interruptions、conversation IDs，并可 `to_state()`：`result.py:481-588`。

`RunResultStreaming` 保存 event/guardrail queues、run task、guardrail tasks、stored exception、interruptions、cancel mode 和 sandbox cleanup：`result.py:594-705`。

取消模式：

- `immediate`：取消 task、清队列、写 completion sentinel；
- `after_turn`：只设 flag，允许当前模型、工具和 Session save 完成。

位置：`result.py:818-864`。调用者 cancel 后仍应继续消费 `stream_events` 完成清理：`result.py:882+`。后台无事件失败可通过 `run_loop_exception` 获取：`:790-816`。

## Turn limit 与错误处理

默认 `max_turns=10`：`run_config.py:44-48`。超限时生成 `MaxTurnsExceeded`；可通过 `error_handlers["max_turns"]` 生成受 output guardrail 验证并持久化的最终输出：`run.py:1441-1539`。流式对应逻辑：`run_internal/run_loop.py:1528-1634`。

Resume 会恢复原 max-turns，调用方不能通过恢复时传参绕过限制：`run.py:2351-2353`。

## Session 与持久化

公共 `Session` protocol 定义 session ID/settings 和异步 get/add/pop/clear：`memory/session.py:16-57`。

内部 persistence：

- pending input admission：`run_internal/session_persistence.py:85-120`；
- history/input merge：`:317+`；
- guardrail trip safe persistence：`:480+`；
- 每轮 result 保存：`:545+`；
- retry/error rewind：`:727+`；
- cleanup await：`:834+`。

运行时区分 client-managed Session 与 server-managed conversation/previous response ID。结果保存 `_current_turn_persisted_item_count`，避免 resume 重复写入：`run.py:2475-2480`。

## RunState、Approval 与 Resume

- `run_state.py:175-218` 定义版本化 schema，当前 `CURRENT_SCHEMA_VERSION="1.17"`，新版本 fail fast。
- `RunResult.to_state()` 保存 agent/context/input、items、responses、guardrails、turn、tool tracker、conversation IDs、trace、pending input、current step 和 sandbox state：`result.py:541-588`。
- Approval interruption 通过 `RunResult.interruptions` 暴露；调用方 approve/reject state 后传回 Runner。
- 恢复路径还原 agent、context、max turns、trace 和 session ownership：`run.py:2297-2353`。
- `turn_resolution.py:1134+` 恢复 interrupted turn，识别 approval、拒绝输出和已存在 tool outputs，防止重复执行。

## 工具并发和 Approval

- 工具 batch executor：`run_internal/tool_execution.py:1552-1641`。
- `max_function_tool_concurrency` 控制并发槽位：`:1643-1665`。
- 任一工具失败时取消可取消兄弟、drain post-invoke、汇总 late failures：`:1667-1714`。
- 父 task 取消时取消 pending 并消费后台 exception，避免泄漏：`:1722-1783`。
- 单工具调用先处理 approval，再执行 body：`:1785-1856`。
- Approval 绑定 namespace、qualified tool key、call ID 和 pending approval，支持 resume 精确匹配：`:1858+`。

## Guardrails 与副作用顺序

- 单 guardrail 使用 span 并记录 tripwire：`run_internal/guardrails.py:31-52`。
- 流式 input guardrails 并发执行；tripwire 时取消/await 兄弟并触发终止 sentinel：`:55-112`。
- 非流式 input/output guardrails：`:115-224`。
- 流式单轮在工具副作用前再次检查 input guardrail，避免 guardrail 与工具执行竞态：`run_loop.py:2035-2050,2324-2343`。
- 结构化 output validation 和 invalid-final-output handler：`turn_resolution.py:1000-1075`。

## Handoff 与历史

- Handoff/tool parsing 位于 `turn_resolution.py`；命名空间工具不会误判为 handoff：`:206-214`。
- Agent span 记录 handoff 和 tool trace names：`run_loop.py:2084-2101,2411-2423`。
- nested history ownership 防止 handoff/resume 后重复 replay：`result.py:81-105,233-252`。
- `to_input_list(preserve_all|normalized)` 提供完整或规范化 continuation history：`result.py:431-453`。

## Tracing 与 Usage

流式 Runner 创建/恢复 trace：`run.py:2407-2421`。run loop 建立 agent/turn/tool spans；usage 包含 retry attempts：`run_loop.py:2260-2288`。敏感 tracing 可由 `OPENAI_AGENTS_TRACE_INCLUDE_SENSITIVE_DATA` 控制：`run_config.py:52-55`。

## 测试覆盖

- Runner：`tests/test_agent_runner*.py`
- Stream/cancel：`tests/test_stream_events.py`、`test_cancel_streaming.py`、`test_soft_cancel.py`
- Guardrails：`test_guardrails.py`、`test_output_guardrail_cancellation.py`、`test_stream_input_guardrail_timing.py`
- Handoff：`test_handoff_tool.py`、`test_handoff_history_duplication.py`
- Approval/resume：`test_tool_approval_call_id_reuse.py`、`test_hitl_session_scenario.py`、`test_runner_guardrail_resume.py`
- Session：`tests/memory/test_session*.py`
- RunState fixtures：`tests/fixtures/run_state/features/*`、`resume/v1_13_pending_tool_approval.json`
- Tracing：`test_trace_processor.py`、`tests/tracing/test_traces_impl.py`

## 优点

- 流式和非流式共享 turn resolution、tool、guardrail、approval 和 persistence 语义。
- Versioned RunState 覆盖 approval、trace、sandbox、max turns 和 pending input。
- 双取消模式区分立即取消与当前 turn 安全收敛。
- Approval 与 tool call identity 精确绑定。
- Guardrail 在副作用前有竞态保护。
- Tool batch 对 sibling cancellation 和 late failure 有清晰处理。

## 风险与限制

- Session 与 RunState 是双层持久化概念，宿主必须正确处理 ownership 和重复写入。
- 仅首 Agent 运行 input guardrail，handoff 后的 Agent 依赖原始决策；产品应明确这一语义。
- cancel 后仍需 drain stream events，调用方容易误用。
- `after_turn` cancel 不是立即停止，UI 必须展示“正在收敛”。
- Tracing 可能包含敏感数据，默认/部署策略需审计。
- 多种 server/client conversation 模式增加 resume 复杂度。

## 对 Kiana 的映射

- `Runner` → Kiana AgentRuntime/ControlPlane。
- `RunResultStreaming` → Kiana event subscription + terminal receipt。
- `RunState` → Kiana durable RunSnapshot（版本化、approval、trace、sandbox、turn）。
- `turn_resolution` → Kiana model-output normalizer + tool/handoff/final state reducer。
- Tool approval identity → Kiana ApprovalChallenge 的 request hash + invocation ID。
- Guardrail `before_side_effects` → Kiana Policy/Gate 在 CapabilityBroker 前的最终检查。
- immediate/after_turn cancel → Kiana `cancel_now` 与 `stop_after_turn` 两种明确命令。

Kiana 应特别测试：cancel 后 event drain、approval call-ID 重用、resume 后不重复 tool、handoff history ownership、guardrail/副作用竞态和 max-turns 跨恢复不被绕过。
