# pydantic-ai — 源码补审

- **状态：** `audited`
- **参考路径：** `reference/pydantic-ai`
- **主要运行时：** `pydantic_ai_slim` 的可外部驱动 Agent Graph
- **审计范围：** `agent, graph, tools, messages, run, result, usage, cancellation, tests`

本报告仅基于源码、测试、manifest 和入口；未读取文档或 Git 历史。

## 总体流程

```text
Agent.run* / Agent.iter
  → GraphAgentState + GraphAgentDeps
  → UserPromptNode
  → ModelRequestNode
  → CallToolsNode
  → ModelRequestNode（工具结果/重试）
  → End(FinalResult)
```

核心不是隐藏的 `while` 循环，而是公开可逐节点推进的状态图。`AgentRun` 包装 `pydantic_graph.GraphRun`，调用方可在节点执行前观察、替换或手动推进。

## 入口与运行对象

- package/CLI：`reference/pydantic-ai/pydantic_ai_slim/pyproject.toml:166-170`，`pai = "pydantic_ai._cli:cli_exit"`。
- `Agent.iter(...)` 是基础入口，`run`、`run_sync` 和流式 API 均建立其上：`reference/pydantic-ai/pydantic_ai_slim/pydantic_ai/agent/__init__.py:1183-1205`。
- `GraphAgentState` 保存 messages、usage、output retry、step、run/conversation ID、metadata、pending messages、event buffer 和 MCP schema cache；`GraphAgentDeps` 保存 model、settings、limits、output schema、validators、capabilities、tool manager、tracer 和 cancellation：`reference/pydantic-ai/pydantic_ai_slim/pydantic_ai/_agent_graph.py:299-439`。
- `AgentRun` 持有 graph run、当前 node、result override 和 node error：`reference/pydantic-ai/pydantic_ai_slim/pydantic_ai/run.py:31-105`。
- hook 顺序：`before_node_run → wrap_node_run → on_node_run_error → after_node_run`：`run.py:323-392`。
- `AgentRun.__anext__` 先 yield 节点，下一次迭代才执行，允许外部检查；每 step 重新绑定 cancellation：`run.py:199-246`。

## Graph 节点职责

| 节点 | 职责 | 位置 |
|---|---|---|
| `UserPromptNode` | 清洗/恢复历史、处理 deferred result、加入 prompt/instructions、决定先处理未完成工具还是请求模型 | `_agent_graph.py:501-705` |
| `ModelRequestNode` | 准备 model/tools/settings，普通或流式请求，追加 response/usage，生成重试或工具节点 | `_agent_graph.py:1106-1814` |
| `CallToolsNode` | 分类 text/image/output/function/deferred tools，生成下一请求或 `End` | `_agent_graph.py:1816-2298` |
| `SetFinalResult` | 流式路径直接终止图 | `_agent_graph.py:2302-2310` |

## 用户输入、Context 与历史恢复

`Agent.iter` 接收 prompt、history、deps、instructions、model/settings、usage limits、cancellation token、metadata、retries、toolsets/capabilities、deferred results、run/conversation IDs：`agent/__init__.py:1183-1205`。

状态初始化：history 被复制；usage 可由调用方带入；run ID 不从历史继承；conversation ID 优先显式值、再取历史、最后生成 UUID7：`agent/__init__.py:1476-1487`、`_agent_graph.py:299-318`。

`RunContext` 将 state/deps 投影为 model、usage/limits、messages、step、IDs、metadata、tool manager、capabilities、pending messages、cancel 和 event buffer：`_agent_graph.py:2338-2375`。

`UserPromptNode` 的恢复规则：

- interrupted request 补合成 tool return；
- 无新 prompt 时可复用尾部 request；
- 尾部 response 有未处理工具时先进入 `CallToolsNode`；
- suspended response 仅允许无新 prompt 时恢复；
- 新 prompt 遇未处理工具会 fail closed；
- 动态 system prompt 在恢复时重算。

位置：`_agent_graph.py:523-658`。

Deferred 恢复验证历史、tool-call ID 和已有 terminal result，拒绝覆盖已执行结果：`_agent_graph.py:660-705`。

## Model 请求、流与 continuation

普通模型请求：

```text
_prepare_request
  → ModelRequestContext
  → capability.wrap_model_request
  → provider model_request
  → on_model_request_error（可恢复）
  → append response + usage
  → limits check
  → CallToolsNode / retry node
```

位置：`_agent_graph.py:1376-1814`。

Provider suspended response 会重新发送 `base_messages + suspended response`，并分别限制 generation continuation 和 background polling；异常/超限时 best-effort 取消远端 job：`_agent_graph.py:911-1041`。

流式路径：

- capability 包裹 model stream；
- continuation 链合并为一个逻辑 stream；
- `stream_ready/stream_done` 协调 hook、consumer 和清理；
- consumer 中断时写入 incomplete/interrupted response；
- event iterator memoize，避免重复 hook。

位置：`_agent_graph.py:1043-1354`。

`AgentStream` 使用共享拉取锁，避免多个 consumer 并发 `anext()`：`result.py:385-442`。

### 流式输出验证限制

中间输出做 partial validation，最终做完整 validation：`result.py:74-101`。最终验证失败不会返回图中重试，而是抛出：

```text
Output validation failed during streaming, and retries are not supported in run_stream()
```

位置：`result.py:244-308`。

## Tool prepare、validate、execute 与 Approval

每 step 的 ToolManager：继承失败工具 retry state、运行 `for_run_step`、缓存可见工具，并写回 RunContext：`tool_manager.py:187-220`。

工具解析同时要求：工具已知、当前 turn 可见、capability 已加载、search-gated schema 已展示：`tool_manager.py:496-550`。

验证顺序：

```text
before_tool_validate
  → wrap_tool_validate
    → Pydantic schema
    → custom args validator
  → on_tool_validate_error
  → after_tool_validate
```

位置：`tool_manager.py:306-428,609-692`。

只有参数验证成功后才能转为 `ApprovalRequired` / `CallDeferred`，防止未经验证的参数进入审批队列。

执行顺序：

```text
before_tool_execute
  → wrap_tool_execute
    → toolset.call_tool
  → on_tool_execute_error
  → after_tool_execute
```

位置：`tool_manager.py:430-494,938-1027`。

执行语义：

- `ModelRetry` 消耗 per-tool retry；
- `ToolFailed` 返回失败结果但不消耗 retry；
- `SkipToolExecution` 仍产生 tool result；
- external tool 不允许本地直接执行；
- 成功后下一 step 清除 retry state。

Approval/deferred 有两种来源：声明式 `unapproved/external`，或 validator/tool 动态抛出。`handle_call` 统一它们：`tool_manager.py:1029-1146`。

未解决的请求形成 `DeferredToolRequests`，后续运行携带 `DeferredToolResults` 恢复：`_tool_execution.py:254-321,374-455`。

## 并发和结束策略

- `early`：首个有效 output 终止，普通工具跳过；
- `graceful`：默认，按 emission order 处理；
- `exhaustive`：执行全部，选择首个有效 output；
- `sequential=True` 建 barrier，其他工具可分段并行。

位置：`_tool_execution.py:254-295`、`tool_manager.py:170-247`。

值得借鉴的 `retry-wins`：任一 function/unknown tool 返回 `RetryPromptPart` 时，已有 final output 被压制，先让模型修复工具问题。

## Output validation 与预算

无 tool call 时，`CallToolsNode` 按 schema 接受 text/image/object；token-length finish 直接报错，content filter 变 `ContentFilterError`，无合法输出则生成 schema-aware retry prompt：`_agent_graph.py:1891-2061,2232-2274`。

Output tool 使用独立 validate/process hooks，不污染普通工具 policy/telemetry：`tool_manager.py:738-936`。

Output retry 与普通工具 retry 独立：`GraphAgentState.output_retries_used` 和 per-tool retry 分开：`_agent_graph.py:361-378`。

`UsageLimits` 支持 request/tool call/input/output/total token、cost、单请求 token 和 preflight count；请求前、响应后、每个流事件、工具批次前分别检查：`usage.py:418-574`、`_agent_graph.py:1603-1621,1723,1788-1794`、`_tool_execution.py:442-448`。

价格不可得时发 `CostNotFoundWarning`，不会假装 cost limit 已强制：`usage.py:516-535`。

## History、持久化与结果

`ModelRequest/ModelResponse` 是可 JSON 持久化历史单位，含 run/conversation ID、metadata、state、parts、usage 和 provider details：`messages.py:1832-1864,2539-2605`。

`ModelMessagesTypeAdapter` 提供 JSON dump/load，用于跨进程恢复和 deferred continuation：`messages.py:2768`。

`AgentRun` 暴露 all/new messages、result、usage、metadata、run/conversation IDs、enqueue 和 cancel：`run.py:144-191,487-560`。

`StreamedRunResult` 提供同类 history/usage/ID surface，并在完成时写入最终 response：`result.py:473-809`。

## Cancellation

- `CancellationToken`：线程安全、可挂多个 run、不可复位；
- `RunCancellation`：每 run 一个，绑定当前 asyncio task，并用 `call_soon_threadsafe` 跨线程取消。

位置：`_cancel.py:42-257`。

每 node step 重新 bind，确保外部手动推进时仍取消正确 task。实现区分自身取消和外部 `CancelledError`；有一个已注释的极窄归因竞态窗口：`_cancel.py:203-242`。

## 测试覆盖

- `reference/pydantic-ai/tests/test_agent.py`
- `reference/pydantic-ai/tests/test_run_cancellation.py`
- `reference/pydantic-ai/tests/test_run_context_usage_limits.py`
- `reference/pydantic-ai/tests/test_usage_limits.py`
- `reference/pydantic-ai/tests/test_agent_output_schemas.py`
- `reference/pydantic-ai/tests/test_tools.py`
- `reference/pydantic-ai/tests/test_toolsets.py`
- `reference/pydantic-ai/tests/test_tool_availability.py`
- `reference/pydantic-ai/tests/test_tool_availability_portability.py`
- `reference/pydantic-ai/tests/test_messages.py`
- `reference/pydantic-ai/tests/test_history_processor.py`

## 优点

- Agent 图公开可驱动，节点和 hook 生命周期清晰。
- 历史恢复会修补/拒绝不闭合工具状态。
- validate-before-defer 保证审批参数已通过 schema。
- Approval、external tool 和 deferred result 使用统一恢复模型。
- 输出 retry、工具 retry、usage budget 相互独立。
- 流中断会保留 incomplete/interrupted history。
- 取消支持跨线程和手动 node 驱动。

## 风险与限制

- `run_stream()` 最终输出验证失败不能自动重试；UI 必须定义已展示 partial output 的撤回/修订语义。
- 该库提供可序列化消息，不等同于完整事务 Session/Run store；宿主仍需持久化 ownership、approval 和 tool side effects。
- 工具并发和 end strategy 较复杂，宿主必须测试副作用顺序。
- provider suspended job 的取消是 best-effort，恢复时需防止重复工具执行。
- cancellation attribution 存在已知极小竞态窗口。

## 对 Kiana 的映射

- `Agent.iter/AgentRun` → `KianaHarness + RunnerPort` 的显式 phase/run state。
- Graph nodes → `request assembly / model / capability / finalize` 可持久化阶段。
- `GraphAgentState` → Kiana RuntimeEvent timeline 与 model transcript 分离。
- ToolManager validate-before-execute → Kiana schema/policy gate 在 broker 前不可绕过。
- DeferredToolRequests → Kiana `ApprovalChallenge`，以 approval ID + request hash + tool call ID 恢复。
- `UsageLimits` → request/tool/token/cost 分层预算，并将价格未知记录为不可强制。
- interrupted/suspended history → Kiana resume 默认 fail closed，避免重放未确认工具。
- streaming validation → Kiana 必须定义 partial output revision event，不能静默替换客户端已见内容。
