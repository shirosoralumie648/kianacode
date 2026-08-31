# ChatDev — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/ChatDev`
- **主要运行时：** 仅依据源码对 ChatDev reference Agent implementation 的审计
- **审计范围：** `runtime, workflow, entity, tools, schema and server execution`

本报告基于源文件、测试、清单以及可执行入口。

## 端到端流程

- 示例请求：用户选择 demo_function_call.yaml，连接 `/ws`，收到 UUID session_id，并以该 session ID 向 `/api/workflow/execute` 发送 task_prompt "What should I wear in Beijing?"。
- 路由要求活跃 WebSocket session，后台启动 `WorkflowRunService.start_workflow`，立即返回 `{status: started}`。服务校验/解析 YAML，创建内存 `WorkflowSession`，发出 workflow_started，并构建 GraphContext/WebSocketGraphExecutor。
- Graph node A 接收规范化 USER input，角色成为 system message；加载 get_city_num/get_weather 的 function tool schemas；OpenAI 同步请求。function_call response 解析为 ToolCallPayload，JSON arguments 解析后调用配置的 Python function，其结果成为 TOOL message 及 function_call_output timeline event。
- 后续 model call 重复，直到没有 tool calls 或达到 tool_loop_limit（默认 50）。A 输出追加到 node A，并沿 A->B 传播。B 收到 A message，以 clothing-advice role 调用模型并发出最终输出。
- GraphExecutor 选择配置的 final/end sink output，记录 node outputs 与 workflow summary，保存 memory，导出 token usage/execution_logs；WorkflowRunService 标记 session completed，并发送带 results/summary/token usage 的 workflow_completed。LaunchView 渲染 logs、node dialogues、artifacts 和 completion。
- 遇到 human node 时，server 发出 human_input_required，并让 worker Future 阻塞；WebSocket message handler 校验/提供输入。收到 cancel 时设置 cancel_event 和 executor cancellation；checkpoint 抛出 WorkflowCancelledError，服务发出 workflow_cancelled。断开连接会保留进程内 session，以便 reconnect replay。

## 组件与边界

- 主要 runtime：FastAPI/uvicorn server + WebSocketManager + YAML GraphExecutor；次级为 sync/SSE route 与 Python SDK/CLI。
- Ingress/UI：`frontend/src/pages/LaunchView.vue`；`server/routes/websocket.py` 与 `server/routes/execute.py`。
- Workflow/entity：`server/services/workflow_run_service.py`、`session_store.py`、`graph.py`、`graph_context.py`、`entity/messages.py`、`entity/configs/node/agent.py`。
- Provider/tools：`runtime/node/executor/agent_executor.py`、`runtime/node/agent/providers/openai_provider.py`、`runtime/node/agent/tool/tool_manager.py`、`entity/tool_spec.py`。
- Persistence/output：`workflow/runtime/result_archiver.py`、`server/services/artifact_dispatcher.py`、`server/services/websocket_logger.py`、frontend event handling。

## 状态、持久化与恢复

- 正常完成时 session status 为 COMPLETED；results 在 TTL GC 前留在内存；graph outputs/logs/token files 写入 `WareHouse/session_<id>`。
- 出错时 status 为 ERROR 并发送 error event；cleanup 清理 executor/graph 与 human-input controller state。
- 取消时 status 为 CANCELLED 并发送 workflow_cancelled；取消是协作式而非强制式。
- 断开时移除活跃 WebSocket，但 session 仍留在进程内以便 reconnect；message_buffer 只重放业务消息，不重放 connection/pong。

## 工具、策略与副作用

- 只允许只读源码探索：find、grep、Read 和 parallel tool calls。
- 未编辑、创建、删除、移动或执行源文件。
- 按请求排除 README.md、CLAUDE.md、AGENTS.md、USER.md、docs、changelogs 和 git history。

## 输出与呈现

- 这是从源码推导的架构，以及从 Vue/WebSocket ingress 经 A tool calls、edge propagation 到 B、final output 和 UI completion 的具体 weather-agent 请求追踪。
- 主要 agent 路径没有 model token streaming：provider calls 同步执行；UI 收到 lifecycle/log events 和 final output，而非 model deltas。
- 普通 configured function/MCP execution 前不存在通用 approval/policy interception；只执行 active skill allowlisting 与 config/spec resolution。
- Persistence/recovery 对 session 是进程内的；output files 在磁盘持久化，但进程重启无法恢复正在运行的 session/thread。

## 测试与验证

- 仅阅读源码测试；未修改文件且未执行测试。
- WebSocket 跨线程投递、并发发送、断开安全和 owner-loop capture：`tests/test_websocket_send_message_sync.py:77-231`。
- Server reload watcher 与 nested exclusion regressions：`tests/test_server_main_reload.py:71-205`。
- 其他测试覆盖附件文件名和 memory implementations，但未发现 YAML ingress -> provider -> tool loop -> edge -> final event 的端到端测试。
- 未发现 Responses API tool-call correlation、tool approval policy、重启后的 session persistence、cancellation race 或 async SDK invocation 的专门源码测试。

## 优势

- server ingress、session services、graph orchestration、node executors、providers 和 tool adapters 分离清晰。
- send_message_sync 的 owner-event-loop scheduling 有专门的并发 worker tests 覆盖。
- YAML filename validation、schema registration bootstrap、model retry policy、bounded message/artifact queues、cancellation checkpoints 与 artifact persistence 都是明确的源码机制。
- Frontend 处理 reconnection snapshots、lifecycle logs、model/tool timing、artifacts、completion、cancellation 和 errors。

## 风险与缺口

- High：WorkflowSessionStore 仅内存存储，进程重启后 reconnect/resume 失败；active execution state 和 pending human Future 无法恢复。
- High：`start_workflow` 无条件为给定 session ID 调用 create_session；重复 execute 请求可能在早期 executor 运行时替换 session record，造成 status/message/result races。
- High：OpenAI Responses follow-up timeline path 在 `_serialize_message_for_responses (:439-451)` 序列化 Message 时不带 tool_calls；tool loop 追加 assistant message 与 FunctionCallOutputEvent，因此 Responses requests 可能缺少关联 output 所需的先前 function-call item。Chat Completions serialization 会保留 tool_calls。
- Medium：AgentNodeExecutor 的宽泛 exception handler 在 input_data 尚未赋值时仍在 error message 中使用它（provider/config/client setup 早期失败即可触发），可能用 UnboundLocalError 掩盖原错误。
- Medium：`agent_executor.py:777-793` 每次 tool call 使用 asyncio.run；从已有 event loop 的线程调用会抛 RuntimeError，影响在 async context 嵌入 SDK/runtime 的调用方。
- Medium：`workflow_run_service.py:35-53` 的 cancellation 立即标记 session CANCELLED，而 worker 可能直到 checkpoint 才停止；并发 completion/error messages 可能与 cancellation state 冲突，frontend 也会乐观更新。
- Medium：断开的非终止 session 会永久留在 store，因为 GC 只删除 COMPLETED/ERROR/CANCELLED（`websocket_manager.py:235-249`），没有 idle RUNNING timeout。
- Medium：`send_message_sync` 等待 owner event loop 最多 10 秒（`websocket_manager.py:165-197`）；阻塞/死亡 event loop 会拖住 worker-side workflow execution。
- Low：主要路径没有 provider/model streaming；尽管 WebSocket UI events 面向 stream，实际只发同步 model completion 和 lifecycle logs。

## 映射到 Kiana

- Ingress：FastAPI WebSocket/HTTP routes 与 Vue LaunchView。
- Session/thread：按 WebSocket UUID 索引的进程内 WorkflowSessionStore；没有 durable database/thread store。
- Prompt/context：GraphExecutor task normalization、AgentNodeExecutor system role/input mode/memory/thinking/skills assembly。
- Model：同步 OpenAI Chat Completions 或 Responses API；Gemini 是次级 provider implementation。
- Tools/schema：ToolSpec、FunctionToolConfig、ToolManager function/MCP adapters、JSON argument parser。
- Policy：普通 tools 在 schema/config 匹配时执行；active Agent Skills 施加 allowed-tools；human input 有显式 approval-like wait，但没有通用 tool approval gate。
- Loop/termination：DAG/Cycle strategies 加 AgentNodeExecutor tool loop limit 50 与 cancellation checkpoints。
- Persistence/recovery：WareHouse graph outputs、summaries、token usage、execution logs、artifact queue；session/message buffer 仅内存，reconnect 仅同进程。
- Events/UI：WebSocketManager、WebSocketLogger、ArtifactDispatcher、LaunchView processMessage。
- Secondary runtimes：`/api/workflow/run` JSON/SSE、`runtime/sdk.py`、`run.py` CLI。

## 源码证据

- Primary ingress/session：`LaunchView.vue:1461-1568,1843-1927`；`server/routes/websocket.py:9-19`；`server/routes/execute.py:13-52`。
- Connection 创建 UUID 或恢复内存 session，重放最多 1000 条 buffered business messages 并发 snapshot：`websocket_manager.py:69-131,140-197`；`session_store.py:23-67,69-157`。
- 具体请求可用 demo_function_call.yaml：A 是在 `:8-31` 配置 get_city_num/get_weather 的 OpenAI weather agent，A->B 与 clothing agent 在 `:33-65`；确定性函数实现位于 `functions/function_calling/weather.py:0-34`。
- Workflow preparation、graph construction、task input、executor run、completion/cancellation/error 和 cleanup：`workflow_run_service.py:55-101,135-205,215-265,267-292`。
- Graph execution 构建 graph/conditions/memory/thinking/node executors，规范化 input，选择 DAG/cycle/majority strategy，收集 outputs、保存 memory、归档 artifacts：`workflow/graph.py:137-141,254-341`；`workflow/runtime/execution_strategy.py:13-148`；`workflow/runtime/result_archiver.py:7-32`。
- Agent prompt/context assembly 与 model/tool workflow：`runtime/node/executor/agent_executor.py:42-174,178-221,223-336,340-431,450-554`。
- Tool calls 被规范化、解析、授权检查、执行，表现为 TOOL messages/function output events，注入 follow-up conversation，循环至完成或 limit 50：`agent_executor.py:558-612,614-848,850-1017,1048-1069`。
- OpenAI provider 选择同步 Chat Completions 或 Responses API，构建 schemas/payloads，追踪 usage，反序列化 tool calls：`openai_provider.py:53-99,167-226,228-286,358-393,432-657`。
- Function/MCP tool discovery/execution：`tool_manager.py:96-173,175-261,278-325`；schema validation/auto-fill：`tooling.py:195-273`。
- Human input/cancellation：`prompt_channel.py:36-121`、`session_execution.py:21-147`、`message_handler.py:23-91`、`workflow/graph.py:103-118`、`runtime/node/executor/base.py:142-145`。
- Event/UI：logs 同步发送到 owner event loop，workspace artifacts 排队广播，frontend 渲染 model/tool/node/artifact/completion/error states：`websocket_manager.py:165-197`、`websocket_logger.py:8-29`、`artifact_dispatcher.py:10-64`、`LaunchView.vue:2095-2334`。
- Secondary paths：`execute_sync.py:34-143,151-252`、`runtime/sdk.py:39-132`、`run.py:76-122`。
- Tests 是 focused regression/unit 而非端到端 agent tests：`test_websocket_send_message_sync.py:77-231`、`test_server_main_reload.py:71-205`、pyproject test discovery/config `:49-60`。
