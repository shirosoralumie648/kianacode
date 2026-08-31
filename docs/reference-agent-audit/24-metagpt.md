# MetaGPT — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/MetaGPT`
- **主要运行时：** MetaGPT
- **审计范围：** `metagpt roles, actions, environment, messages and team run loop`

本报告基于源文件、测试、清单以及可执行入口。

## 端到端流程

- 具体请求：`metagpt "create a 2048 game"` 进入 Typer `startup`，随后 `generate_repo` 创建 Context/Team 并雇用 Mike、Alice、Bob、Alex、David（`software_company.py:39-71`）。
- `Team.run` 将 human requirement 发布为 Message（`team.py:121-126`）；MGXEnv 使 public messages 面向所有 roles，但将普通 role outputs 路由经 Mike（`mgx_env.py:17-61`）。Environment 将匹配消息投递到 private role queues，并并发运行非 idle roles（`base_env.py:174-210`）。
- Mike 的 `RoleZero.run` 观察 requirement、写入 memory 并调用 `_react`；快速分类可直接回答，否则 `_think` 构建 role/team instructions、plan status、available tool schemas、recent history 与 user command prompt（`role_zero.py:197-264`）。
- LLM call 由 system messages 加格式化 memory/user messages 组装（`base_llm.py:178-209`），通过 OpenAI-compatible async completion 发送；stream chunks 被打印/排队，usage/cost 更新（`openai_api.py:92-136`）。
- RoleZero 期待 JSON command text，而不是 API-native tool-use loop。`parse_commands` 提取/修复 JSON 并强制 exclusive commands（`role_zero_utils.py:98-144`）；`_run_commands` 顺序调用映射方法、捕获失败，并将文本 output UserMessage 注入 memory（`role_zero.py:279-300,384-414`）。
- Mike 可调用 `TeamLeader.publish_team_message`，重置自身 state 并向 Alice/Bob/Alex/David 发送 UserMessage（`team_leader.py:74-85`）。响应经 MGXEnv/Mike 返回；每个 role 重复 observe -> think -> act，直到 state/end/no news/max loop。Engineer2 可调用 `Engineer2.write_new_code`：第二次 LLM request 生成 code，CodeParser 提取，`awrite` 持久化，EditorReporter 报告 metadata/path（`engineer2.py:111-141`）。
- 完成由 model-command 驱动：`Plan.finish_current_task` 标记 task；`end` 调用 `_end`，确保 human reply（额外 LLM call）并可选总结（`role_zero.py:419-490`）。Role 发布 final AIMessage；Team 循环直到 rounds、idle 或 budget，然后 archive（`role.py:529-559`、`team.py:127-137`）。
- Resume 是 filesystem recovery，不是 session/thread resume：读取 `team.json`，恢复 context，重建 Team（`team.py:58-80`）；transient role todo/news/buffer 排除，最新 observed state 支持重新观察（`test_team.py:111-147`、`role.py:398-418`）。
- Flask 次级 ingress 每个请求启动新的 background thread 与 Role，在线程存活时 yield StreamPipe output；没有 resume/session/cancel protocol（`examples/stream_output_via_api.py:31-90`）。

## 组件与边界

- 主要 Python console runtime：`.../MetaGPT/metagpt/software_company.py` -> `Team` -> `MGXEnv`/`Environment` -> `Role`/`RoleZero` -> provider/action/tool layers。
- 次级 Python Flask streaming adapter：`.../MetaGPT/examples/stream_output_via_api.py`；不是 session service，只暴露一个 stream endpoint。
- Provider layer 基于 OpenAI-compatible 且支持多 provider，通过 `.../provider/base_llm.py` 和 `openai_api.py` 实现。
- 持久 runtime state 为 Team JSON 加 Pydantic role/context/memory state；没有 first-class session/thread object。

## 状态、持久化与恢复

- Team fields：`env`、`investment`、`idea`、`use_mgx`；role state：`RoleContext.state`、`todo`、`watch`、`react_mode`、`max_react_loop`、private message queue、memory、working memory；planner state 由 role 拥有。
- Message state 包括 generated UUID、content、role、cause_by、sent_from、send_to、metadata；environment 保存 debug history，各 role 保存过滤后的 memory。
- Reporter state 包括 UUID、block type、callback URL、LLM stream queue task 和 end marker；Flask state 是指向 StreamPipe 的 ContextVar 加未管理的 `threading.Thread`。
- 审计 runtime 源码未出现 `session_id`、`thread_id`、resume token、event cursor、approval record、cancellation flag 或 durable in-flight tool-call state。

## 工具、策略与副作用

- 仅进行 filesystem source/test inspection，未修改文件。
- 使用 `Read` 读取主要证据，使用只读 `find`/`grep` 发现 symbol/file。

## 输出与呈现

- 审计只基于源码，排除 README.md、CLAUDE.md、AGENTS.md、USER.md、docs、changelogs 和 git history。
- 单一请求可沿主要 console runtime 端到端追踪，但源码没有 canonical session/thread ID、durable conversation object、approval policy engine、tool-result API round trip 或 cancellation endpoint。
- 重要限制：Team serialization decorator 捕获异常并序列化但不重新抛出（`common.py:675-686`）；Role decorator 在重新抛出前删除最新 observed message（`common.py:689-716`）。未显式处理 `asyncio.CancelledError`，因此 cancellation semantics 与普通错误不同。
- 重要架构区别：RoleZero dynamic loop 消费 LLM 生成的 JSON command text 并执行本地 Python methods；`BaseLLM` 的 API-native function-call parsing 是 `aask_code` 使用的独立 helper，不是主要 RoleZero command loop。
- 本地副作用前没有 native tool approval。Human interaction 是协作式/模型选择的（`ask_human`/`reply_to_human`）；exclusivity 只限制重复 file-edit commands。

## 测试与验证

- `tests/metagpt/test_software_company.py:17-36` CLI/company smoke coverage。
- `tests/metagpt/test_environment.py:71-88` environment message publication 与 concurrent processing。
- `tests/metagpt/roles/di/test_role_zero.py:24-39` RoleZero model validator 与 think/react cycle。
- `tests/metagpt/roles/di/test_team_leader.py:39-173` delegation、routing、plan update 与 human progress response。
- `tests/metagpt/provider/test_base_llm.py:45-105` message formatting 与 API-native function argument parsing。
- `tests/metagpt/test_reporter.py:44-53,156-179` callback events 与 LLM stream reporter。
- `tests/metagpt/serialize_deserialize/test_team.py:71-147` save/recover loops 与 excluded transient state。
- `tests/metagpt/serialize_deserialize/test_role.py:90-111` role interruption/recovery behavior。

## 优势

- Team orchestration、Environment routing、Role cognition、Action execution 与 provider abstraction 分离清晰。
- Pydantic polymorphic serialization 支持 role/action reconstruction，且 memory/state recovery 有测试。
- Async environment execution 并发运行 roles，而 RoleZero 顺序执行 commands 以保持 command order。
- Reporter abstraction 发出带稳定 UUID 和明确 stream end markers 的结构化 task/tool/file/thought events。
- 测试覆盖 CLI smoke、environment routing、RoleZero cycle、TeamLeader delegation、provider parsing、reporter callbacks 以及 Team/Role recovery。

## 风险与缺口

- 没有 first-class sessions/threads；并发请求只有在显式配置时才可能共享 filesystem/project state，Flask threads 没有 cancellation 或 lifecycle registry。
- Primary team run 有 round/cost 上限，却不是 event-driven completion；即使仍有 pending messages，`Team.run` 也可能在 n_round 归零时停止。
- Tool failures 会停止当前 batch 的剩余 commands 并变成 traceback text，需由 LLM 解释；没有 structured error/result envelope 或 approval audit。
- Serialization 只保证捕获普通 exception/KeyboardInterrupt 时执行，且使用排除的 transient fields；cancellation、process death 与 in-flight model/tool recovery 没有明确持久化。
- MGXEnv 中 human reply 是固定 acknowledgement（`reply_to_human`），实际内容只经 reporter/UI side channels 发送；消费者必须用 reporter UUID 关联。
- Flask adapter 接受任意 request data，无 auth/session isolation，并为每个请求启动一个未管理线程。

## 映射到 Kiana

- Ingress -> `software_company.startup` / Flask `/v1/chat/completions`
- Session/thread -> primary 中不存在；Flask 每次请求创建未管理 Python thread
- Context -> Pydantic `Context` 加 role memory/planner/working memory
- Prompt assembly -> `RoleZero._think` + `BaseLLM.aask`
- Model request/stream -> `BaseLLM` -> `OpenAILLM`；token logging 经 `logs.py` queue
- Tool-call parsing -> `role_zero_utils.parse_commands` 的 JSON text parser；API-native function parsing 独立存在
- Policy/approval -> 没有正式 approval/policy gate；仅模型选择的 human commands 和 command exclusivity
- Tool execution -> RoleZero `tool_execution_map`、special commands、Engineer2 tools
- Tool result injection -> 带 `cause_by=RunCommand` 的 UserMessage 注入 role memory
- Loop/termination -> Team rounds/budget + role max_react_loop + plan/end commands + idle detection
- Persistence/recovery -> `Team.serialize`/`deserialize`、Context serialization、role memory/state recovery
- Event/UI -> Loguru stream hooks、ResourceReporter HTTP callbacks、Flask StreamPipe
- Cancellation/errors -> KeyboardInterrupt/Exception decorators；没有显式 request cancellation 或 session lifecycle

## 源码证据

- Ingress/team construction：`software_company.py:13-73,76-123`；console entrypoint manifest：`setup.py:97-119`。
- Team run loop 与 budget/termination/archive：`team.py:91-137`；environment routing 与 concurrent role execution：`environment/base_env.py:155-210,227-247`。
- Message shape、IDs、cause/sender/recipient routing、serialization：`schema.py:231-337`；memory insertion/news detection：`memory/memory.py:19-38,79-91`。
- Role observe/react/action path：`roles/role.py:339-396,398-426,453-469,512-559`；prompt stage selection template：`role.py:50-78,358-377`。
- RoleZero model-driven path：`roles/di/role_zero.py:197-273,279-335`；tools/commands：`role_zero.py:72-87,117-170`；TeamLeader routing：`team_leader.py:21-85`；Engineer2 code-writing：`engineer2.py:30-105,111-141`。
- Command extraction/repair/exclusive policy：`utils/role_zero_utils.py:68-95,98-144`；sequential tool execution、traceback capture 与 result injection：`roles/di/role_zero.py:384-414`。
- LLM prompt assembly、message compression、request dispatch：`provider/base_llm.py:93-115,178-209,339-379`；OpenAI request/stream/retry：`provider/openai_api.py:92-136,138-177`；API-native function argument parsing/fallback：`openai_api.py:230-263`、`base_llm.py:275-313`。
- MGX public/direct routing 与 sender/recipient content injection：`environment/mgx/mgx_env.py:17-61,72-95`。
- Event/UI reporting、streamed token queue：`utils/report.py:48-180`；log hooks/queue/human input：`logs.py:57-72,85-115,127-153`；reporter tests：`tests/metagpt/test_reporter.py:44-53,156-179`。
- Human interaction/termination：`roles/di/role_zero.py:419-446,455-490`；default MGX input/reply：`environment/mgx/mgx_env.py:63-70`。
- Persistence/recovery：`team.py:58-80`；Context state：`context.py:101-126`；exception serialization/role recovery deletion：`utils/common.py:675-716`；recovery tests：`tests/metagpt/serialize_deserialize/test_team.py:71-147`、`test_role.py:90-111`。
- Flask adapter ingress/thread/output：`examples/stream_output_via_api.py:21-90`；其在 76-80 行拒绝 non-stream requests，在 81-88 行只消费最后一条 user message。
- Coverage tests：`tests/metagpt/test_software_company.py:17-36`、`test_environment.py:71-88`、`roles/di/test_role_zero.py:24-39`、`roles/di/test_team_leader.py:39-173`、`provider/test_base_llm.py:45-105`。
