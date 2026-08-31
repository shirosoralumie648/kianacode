# adk-python — 源码补审

- **状态：** `audited`
- **参考路径：** `reference/adk-python`
- **主要运行时：** Python `Runner` + `InvocationContext` + Session Event queue
- **审计范围：** `src/google/adk/runners.py, agents, events, sessions, plugins, tools, tests`

## 端到端流程

```text
Runner.run_async
  → Session load/create
  → InvocationContext
  → user Event
  → root Agent/Node task
  → InvocationContext event queue
  → Runner plugin/persist/yield
  → model/tool events
  → next Agent/Node
  → final/pause/resume/cancel
  → Session event history + stream output
```

入口：`reference/adk-python/src/google/adk/runners.py:1215-1468`；CLI module：`src/google/adk/cli/__main__.py:19`、Click main：`src/google/adk/cli/cli_tools_click.py:362`。

## Session 与 InvocationContext

Runner 接收 `user_id/session_id/new_message`，可带 invocation ID、state delta 和 run config；无 role 的消息补为 user：`runners.py:1215-1263`。

Session 缺失时只有 `auto_create_session=True` 才自动创建：`runners.py:1082-1122`。

`InvocationContext` 持有 session、services、plugins、agent/node path、branch/isolation、agent checkpoints、event queue、active tools、run config 和 cancellation：`agents/invocation_context.py:105-275`。

用户消息进入 Session 的 user Event，并附加 state delta、branch/isolation：`runners.py:922-956`。

## Event Queue 与最终输出

Node/Agent 生产 Event，Runner 是唯一消费、执行 plugin、持久化和 yield 的消费者：`runners.py:721-745,973-1018`。

`InvocationContext._enqueue_event()`：

- partial event：只入队、不等待持久化，用于流式输出；
- non-partial event：写入 Session 后才解除生产方等待。

位置：`agents/invocation_context.py:293-315`。

Event 核心字段包括 invocation ID、author、content、actions、output、node info、long-running tool IDs、branch 和 isolation：`events/event.py:91-155`。

`is_final_response()` 并不等同于整个 invocation 完成；多 Agent 中多个 Agent 都可能产生 final Event：`events/event.py:288-304`。Task mode 会把 task result 提升到 terminal Event output：`runners.py:1248-1253`。

## Agent、Model、Tool、Plugin

`BaseAgent.run_async()` 依次运行 before callback、Agent implementation、after callback；异常触发 on-error callback 后重抛：`agents/base_agent.py:300-340`。

PluginManager 覆盖 user message、run、event、agent、model、tool 的 before/after/error 生命周期；普通 callback 按注册顺序执行，首个非 None 结果短路，error notification 则 best-effort 通知全部插件：`plugins/plugin_manager.py:40-57,134-379`。

Event plugin 在持久化前运行，保证外部收到的 Event 与 Session Event 对齐：`runners.py:1720-1746`。

EventActions 同时表达 state/artifact delta、agent transfer、auth/tool confirmation、agent checkpoint、workflow route 和 rewind：`events/event_actions.py:78-202`。

## Persistence、Pause、Resume、Cancel、Rewind

Session 数据模型为 `{id, app_name, user_id, state, events, last_update_time}`：`sessions/session.py:28-73`。

`BaseSessionService.append_event()` 跳过 partial，应用 temp state 到当前 invocation，移除 temp key，再投影普通 state delta 并 append Event：`sessions/base_session_service.py:166-226`。InMemory backend 明确只适合测试/开发，不能用于多线程生产：`sessions/in_memory_session_service.py:61-66`。

Resume 可由显式 invocation ID 或历史 FunctionCall/FunctionResponse ID 推导；恢复时重建 user content、agent states 和 checkpoint：`runners.py:532-568,881-920,2311-2375`、`invocation_context.py:370-402`。

long-running tool 使 Agent pause，但不等于结束；顺序依赖节点会暂停，parallel sibling 不一定立即停止：`invocation_context.py:495-544`。

调用方提前停止 async event stream 会取消未结束 root task：`runners.py:1020-1041`。生产宿主必须明确“断开订阅”和“取消执行”是否相同。

Rewind 不删除历史，而是追加新的 rewind Event，并用反向 state/artifact delta 表达恢复：`runners.py:1470-1624`。

## Tool Approval / HITL

`ToolConfirmation` 保存 hint、confirmed、payload；请求写入 `EventActions.requested_tool_confirmations[function_call_id]`：`tools/tool_confirmation.py:28-56`、`events/event_actions.py:152-156`。

Approval processor：

```text
当前 branch 最后 user Event
  → 找 approval FunctionResponse
  → 删除已消费 approval
  → 找原 FunctionCall
  → 校验 tool、call ID、name、args 与历史一致
  → 重执行同一原始调用
```

位置：`flows/llm_flows/request_confirmation.py:75-214,255-368`。

这是强安全模式：审批不能替换成另一个工具或参数。测试覆盖 static/dynamic approval、approve/reject、pause/resume、sequential/parallel：`tests/unittests/runners/test_run_tool_confirmation.py:141-1005`。

## 测试覆盖

- 主 Runner：`tests/unittests/test_runners.py`
- Node runtime：`tests/unittests/runners/test_runner_node.py`
- pause：`test_pause_invocation.py:121-520`
- resume：`test_resume_invocation.py:63-425`
- approval/HITL：`test_run_tool_confirmation.py:141-1005`
- rewind：`test_runner_rewind.py`
- session service：`tests/unittests/sessions/test_session_service.py`
- plugins：`tests/unittests/plugins/test_plugin_manager.py`

## 优点

- Event 是对话、工具、审批、状态、checkpoint 和 rewind 的共同事实日志。
- 单消费者 queue 保证 non-partial Event 持久化后才继续。
- Approval 绑定原始 function-call ID/name/args 并去重。
- temp state 可在 invocation 内共享但不污染长期 Session。
- pause/resume 覆盖顺序、并行、嵌套并行和 loop。

## 风险与缺口

- Node Runtime 与旧 BaseAgent 兼容路径双轨并存，事件/错误/partial 时序需保持一致。
- Agent-level final Event 不是 invocation-level completion。
- 自定义 SessionService 必须实现 optimistic concurrency，否则可能 lost update。
- 提前停止 stream 等价于取消执行，断线策略必须显式定义。
- Plugin 首个非 None 短路产生顺序依赖。
- InMemorySessionService 不能作为生产 backend。

## 对 Kiana 的映射

- Runner → Kiana RuntimeRunner/ControlPlane；
- InvocationContext → Kiana RunContext，含 invocation ID、session snapshot、branch、checkpoint、abort；
- Event/EventActions → Kiana append-only RuntimeEvent；
- event queue → Kiana EventDispatcher/SessionEventWriter，non-partial 写成功后才继续；
- BaseSessionService → Kiana SessionRepository + revision/OCC；
- ToolConfirmation → Kiana ApprovalRequirement，绑定 toolCallId、canonical tool name/args；
- rewind → Kiana compensation/rewind Event，不删除历史。

Kiana 必测：non-partial Event 先落库、approval 不可换调用、stale write 不可覆盖、断线/取消语义分离、Agent final 与 invocation complete 分离、partial Event 不改变持久 state。
