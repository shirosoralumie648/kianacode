# codex — 源码补审

- **状态：** `audited`
- **参考路径：** `reference/codex/codex-rs`
- **主要运行时：** Rust `CodexThread` / `Session` / `run_turn`，由 CLI/TUI/app-server 适配
- **审计范围：** `cli, tui, app-server, core/session, tools, approval, persistence, tests`

## 总体流程

```text
CLI / TUI                            app-server JSON-RPC
  → run_interactive_tui                → ClientRequest::TurnStart
  └──────────────┬──────────────────────┘
                 → CodexThread / SessionIo submission
                 → Session handlers / RegularTask
                 → session::turn::run_turn
                 → context + history + tools
                 → Responses/model stream
                 → assistant text 或 tool call
                 → policy/sandbox/approval
                 → executor/MCP
                 → history/result reinjection
                 → next sampling
                 → TurnComplete / TurnAborted / suspend
```

`CodexThread` 是核心边界：上层输入成为 Session submission；Session 持有 history、settings、events、tasks 和 cancel state；`run_turn` 驱动模型和工具闭环。

## CLI、TUI 与 App Server Ingress

- CLI main 通过 `arg0_dispatch_or_else` 进入 async 主函数：`reference/codex/codex-rs/cli/src/main.rs:1040-1046`。
- 解析 `MultitoolCli`、config/feature override；无子命令或 `agents` 进入 TUI，`exec/review` 走非交互路径：`cli/src/main.rs:1048-1160`。
- TUI 用户提交被抽象为 `ClearUiAndSubmitUserMessage` / `SubmitUserMessageWithMode`：`tui/src/app_event.rs:354-356,1153-1158`。
- TUI dispatch 将消息送入 chat widget/session，并处理长度、mentions 和 attachments：`tui/src/app/event_dispatch.rs:163-168,2552-2558`、`chat_composer.rs:336-338,2982-2985`。
- App Server 将 JSON-RPC 反序列化为 `ClientRequest`：`app-server/src/message_processor.rs:104-107,596-830`。
- ThreadStart/Resume/Fork/TurnStart 分发到处理器：`message_processor.rs:1116-1158,1440-1458`。
- `turn/start` 校验 thread、input 和 settings，构造 options 并启动核心 turn：`app-server/src/request_processors/turn_processor.rs:179-225,478-613`。

## Thread、Turn 与并发控制

- `CodexThread` 持有 Session、SessionIo、rollout 路径和 OOB elicitation：`core/src/codex_thread.rs:202-245`。
- `submit/submit_with_trace` 进入 session submission channel：`:247-249,321-331`。
- 三种互斥输入语义：
  - `start_or_steer_turn`：idle 启动，running 时 steer；
  - `start_turn_if_idle`：仅 idle 启动；
  - `steer_turn`：仅活动 turn ID 匹配时注入。

位置：`codex_thread.rs:333-464`。

非 steer 启动前做 agent-control 容量检查：`:466-479`。`RegularTask::run` 循环调用 `run_turn`，支持工具/模型后续步骤：`core/src/tasks/regular.rs:44-84`。核心入口接收 user input、TurnContext 和 CancellationToken：`core/src/session/turn.rs:139-216`。

## Context 与模型请求

- 每 step 通过 `capture_step_context` 获取当前动态上下文；首采样、后续采样和 compact 前后都会重新捕获：`session/turn.rs:301-389,1016-1156`。
- Prompt 由 history、model-visible tools、instructions 和可选 JSON schema 组成：`:1311-1328`。
- 后续采样从 Session history 按模型 modalities 过滤，并附加未进入 prompt 的已执行工具结果：`:1368-1382`。
- Thread snapshot 保存用户/assistant history、settings、environment、model、approval 和 permission/sandbox profile：`codex_thread.rs:78-130`。
- `run_sampling_request` 创建 `ToolCallRuntime`、code-mode worker 和 retry state：`session/turn.rs:1340-1399`。
- context overflow、usage limit 和 retryable stream errors 分开处理：`:1401-1439`。
- `built_tools` 组装 MCP、plugins、connectors、apps 和 extension step tools：`:1494-1570`。

## Stream、事件与 UI

- core events 覆盖 assistant text/reasoning、tool 生命周期、approval、compaction、error、TurnComplete/Aborted：`session/turn.rs:1750-1848`。
- plan mode 有独立 started/delta/completed 解析和缓存：`:1579-1717`。
- App Server 将 TurnStarted 和 TurnAborted 映射为 notifications，并协调 pending interrupt response：`app-server/src/bespoke_event_handling.rs:159-192,1114-1123,1518-1590`。
- TUI 按 turn ID FIFO 处理 request_user_input 和审批：`tui/src/app/app_server_requests.rs:78-93,396-428`。

## Tool Call、Policy、Approval 与回灌

- 每次 sampling 创建 `ToolCallRuntime`，绑定当前 step/turn 和 diff tracking：`session/turn.rs:1353-1358`。
- 工具结果通过 Session history 和 `executed_tool_calls.attach_pending_to_prompt(...)` 进入下一模型请求：`:1368-1382`。
- MCP、shell、patch、web search 和 dynamic tool 均映射为统一 session events：`:1800-1818`。

Thread config 保存：

```text
approval_policy
approvals_reviewer
permission profile
active permission profile
sandbox policy
```

位置：`core/src/codex_thread.rs:78-129`。

Exec approval 由 session handler 转换并通知等待者；带 policy amendment 的批准会持久化 amendment：`core/src/session/handlers.rs:172-201`。Patch approval 使用同一 session-level 通道：`:205-210`。

审批是 core runtime 状态，不是 UI 装饰；UI/TUI/app-server 只负责显示和提交 decision。

## 取消、Interrupt 与 Suspend

- RegularTask/turn/sampling 使用 child CancellationToken，传到 prewarm、context capture、model stream 和 tools：`tasks/regular.rs:44-84`、`session/turn.rs:153-216,1340-1399`。
- `run_turn` 将取消转换为 `TurnAborted`：`session/turn.rs:173-216,469-553,2755-2761`。
- App Server `turn_interrupt` 先验证 active turn 并登记 pending request，正常 interrupt 的响应延后到 TurnAborted；startup interrupt 立即确认：`turn_processor.rs:1474-1533`。
- Thread suspend 语义不同：停止 root turn，但故意不写 TurnAborted/TurnComplete，使另一 worker 可用相同 turn ID 恢复；取消、flush、writer close 完成前不转移 ownership：`codex_thread.rs:405-443`。

## Persistence、Recovery 与 Rollback

- CodexThread 暴露 history/thread read、metadata update 和 rollout append，并要求 live thread persistence：`codex_thread.rs:683-739`。
- Turn recovery 保留原 turn ID、不产生新 user input，仅在 idle 时启动：`:370-402`。
- recovery submission 走 `turn_input::handle_recovery`：`session/handlers.rs:584`。
- Session 模式更新写入 live thread；退出时 flush 并关闭 persistence：`session/handlers.rs:365-385,451-477`。
- rollback 先 flush，重建 rollout/history，写 rollback event，再 flush：`:278-346`。
- rollout reconstruction 反向扫描最新 replacement-history checkpoint、in-progress turn 和 resume/fork metadata：`session/rollout_reconstruction.rs:8-36,114-186,293-294`。

## 测试覆盖

| 主题 | 关键测试 |
|---|---|
| App-server ingress/RPC/thread/turn events | `app-server/src/message_processor_tracing_tests.rs:468-700` |
| turn start/interrupt/thread lifecycle | `request_processors/thread_processor_tests.rs`、`turn_processor.rs:1474-1533` |
| interrupt event mapping | `bespoke_event_handling.rs:3697-3722` |
| resume/compaction/incomplete turn | `core/src/session/rollout_reconstruction_tests.rs:79-2006` |
| tool/context/stream | `stream_events_utils_tests.rs`、`mcp_tool_call_tests.rs`、`compact_tests.rs` |
| TUI pending input/replay | `tui/src/app/tests.rs:1004-1113`、`pending_interactive_replay.rs:696-943` |
| TUI approval/request FIFO | `tui/src/app/app_server_requests.rs:648-1016` |

## 优点

- Core 是唯一执行控制面，CLI/TUI/app-server 是薄适配器。
- Thread/Turn/step 和 start/steer/recover/suspend 语义清晰。
- 每次 step 重捕获 context，历史/工具结果能精确回灌。
- Approval 与 permission/sandbox profile 位于 Session/Core。
- CancellationToken 贯穿 model/tool/context，并区分 interrupt 与 suspend。
- Rollout reconstruction、rollback 和 flush 形成可靠恢复边界。
- 统一事件流支持多种 UI/协议。

## 风险与注意点

- Core/runtime 规模很大，Thread/Session/rollout/tool/event 的兼容演进需要强测试。
- Suspend 不产生 terminal event，消费者必须识别 ownership transfer 状态，不能无限等待 TurnComplete。
- UI/app-server pending request 与 core event 的关联必须保留 turn ID，防止 stale response。
- Context、tools、plugins 和 extension store 动态变化，恢复时必须固定或记录配置版本。
- 多 transport 输出可能有背压/丢事件，需要 terminal reconciliation 和 history backfill。

## 对 Kiana 的映射

| Codex | Kiana |
|---|---|
| `CodexThread` | `AgentSession` / durable Session aggregate |
| `SessionIo submission` | `AgentApplicationService` command queue |
| `TurnContext` / `run_turn` | `RunContext` / single-agent runtime state machine |
| `ToolCallRuntime` | `CapabilityInvocation` + broker |
| approval policy/profile | Policy/Gate/ApprovalStore |
| Responses events | canonical RuntimeEvent |
| rollout/live thread | EventStore + snapshot/projection |
| TurnAborted | terminal cancel event |
| suspend/recovery | worker lease transfer / resumable Run |

Kiana 应优先吸收：Thread/Turn 分离、start/steer/recover 互斥语义、每 step context capture、工具结果 history attach、core-owned approval、CancellationToken 全链传播、rollout reconstruction 和 terminal event correlation。
