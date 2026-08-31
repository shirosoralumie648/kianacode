# mini-swe-agent — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/mini-swe-agent`
- **主要运行时：** mini-swe-agent 2.4.6；主要运行时是由 LitellmModel + LocalEnvironment + InteractiveAgent 支持的 Typer `mini`/`mini-swe-agent` 命令。仅源码审计；未打开被禁止的文档和 git 历史。
- **审计范围：** `src agent loop, environment, model, commands and tests`

本报告基于源文件、测试、清单和可执行入口。

## 端到端流程

- 具体请求：`mini --task "echo hi then finish"` 进入 Typer main，合并 mini.yaml 和 CLI 覆盖项，选择 LitellmModel、LocalEnvironment 和 InteractiveAgent，然后调用 run（mini.py:53-104）。没有 `--task` 时，prompt_user.py:6-19 提供多行提示/历史 UI；此历史是输入历史，不是 agent 会话。
- DefaultAgent.run 保存任务变量，并使用 Jinja StrictUndefined 渲染系统和实例模板，然后由 model.format_message 创建初始 system/user 消息（default.py:87-94；mini.yaml:1-18）。InteractiveAgent.add_messages 使用 Rich 打印每条消息（interactive.py:42-55）。
- 每次迭代检查步骤/成本/墙上时钟限制，递增 n_calls，并将完整的内存消息列表传给 model.query（default.py:125-151）。LitellmModel 使用 bash 工具调用 litellm.completion；retry.py:8-24 对非中止失败按配置的尝试次数以指数等待重试。主要路径没有 token 流式传输或流事件回调。
- 响应解析器要求至少一个 bash 工具调用、JSON 对象参数、已知工具名称以及 command 字段（actions_toolcall.py:29-75）。有效助手消息会保存已解析操作、原始响应、成本和时间戳。FormatError 将成本/响应保存到中断消息中，并重新抛出（litellm_model.py:80-105）。
- InteractiveAgent 根据模式和正则白名单检查所有命令，在 confirm 模式下请求批准，或在 yolo 模式下立即执行（interactive.py:161-180）。LocalEnvironment 通过 shell=True Popen 顺序执行每个操作，将 stderr 合并到 stdout，返回输出/returncode，并报告超时详情；准确匹配第一行完成标记时，会以剩余输出作为提交内容抛出 Submitted（local.py:23-55, 71-91）。
- 普通操作会转换成 user/tool 观察结果，并在下一次模型查询前追加（default.py:153-156；actions_toolcall.py:78-112）。完成时，InteractiveAgent 的 finally 块仍会格式化填充的 `action was not executed` 观察结果，然后 Submitted 继续传播；DefaultAgent 捕获 InterruptAgentFlow，追加退出消息，保存，检测 role=exit，并返回其额外 payload（interactive.py:123-159；default.py:95-123）。
- 交互步骤中的 KeyboardInterrupt 会转换成用户中断提示（interactive.py:108-121）。批处理运行器只取消尚未开始的 future；正在运行的模型调用或环境进程没有协作式取消（swebench.py:263-270）。其他异常会被记录为退出消息，保存，然后重新抛给调用者（default.py:116-121）。

## 组件与边界

- 主要 CLI 运行时：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/run/mini.py
- Agent 循环/策略：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/agents/default.py 和 interactive.py
- 本地执行：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/environments/local.py
- 模型/工具适配器：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/models/litellm_model.py 和 models/utils/actions_toolcall.py
- 次要运行时：hello_world.py、swebench.py、swebench_single.py、programbench.py、inspector.py

## 状态、持久化与恢复

- 内存中的对话是 `DefaultAgent.messages`；每次模型请求都会发送完整的准备好后的历史。主要 Litellm 聊天路径不使用外部线程或服务端响应 ID。
- 计数器为 `n_calls`、`cost`、`n_consecutive_format_errors` 和墙上时钟开始时间；限制通过派生自 InterruptAgentFlow 的异常终止。格式错误可以循环，直到达到阈值，然后追加 RepeatedFormatError。
- 持久化发生在 agent 循环每一步之后的 finally 块中，包括中断/错误路径；批处理运行器还会写入每个实例的轨迹和预测文件（run/benchmarks/swebench.py:121-176）。

## 工具、策略与副作用

- Read、Bash（只读 find/grep）、multi_tool_use.parallel、ToolSearch、SendMessage、StructuredOutput

## 输出与呈现

- 主要跟踪从 CLI 入口到最终返回的 `{exit_status, submission}` 完整覆盖：配置 -> 提示/上下文 -> 模型请求 -> 解析器 -> 批准 -> 环境 -> 观察结果注入 -> 循环/终止 -> 保存/UI/错误。
- 保存的轨迹包含消息历史以及模型/环境/agent 配置元数据（`trajectory_format: mini-swe-agent-1.1`），但它是检查工件，不是可恢复的会话。

## 测试与验证

- 源码测试基于 pytest，覆盖默认循环行为、限制、超时、输出捕获、观察结果、时间戳、格式错误阈值、工具解析器契约、交互式策略、CLI 连接、轨迹创建和保存 schema。
- 未覆盖：真实会话/线程创建或恢复（未实现）、原子持久化/恢复、模型流式传输/取消、全局调用限制执行，以及无 mock 的端到端实时提供商执行。

## 优点

- 小型的基于协议的架构清晰分离 agent 循环、模型适配器和环境；工厂允许替换适配器而无需修改循环。
- 工具调用验证明确，并针对缺少调用、格式错误的 JSON、未知工具和缺少 command 字段进行了测试。
- 超时处理保留部分输出并终止 POSIX 进程组；完成标记处理在本地/容器环境之间共享。
- FormatError 持久化会有意保存模型响应数据，并在 JSON/repr 之间回退；测试验证原始 FormatError 仍会传播。
- 交互式测试涵盖确认、拒绝恢复、模式变更、yolo、human 模式、帮助和键盘中断。

## 风险与缺口

- 源码中不存在会话/线程身份、反序列化、恢复或恢复执行路径。Inspector 只会重新读取 JSON（run/utilities/inspector.py:152-175, 277-284）。重新运行会启动新的进程/agent；复用同一个实例会重置消息，但不会重置计数器/开始时间/格式错误状态（agents/default.py:41-49, 87-94）。
- 轨迹写入使用 mkdir 加普通的 Path.write_text JSON，没有原子临时文件/重命名或 fsync（agents/default.py:181-189），因此中断或磁盘故障可能留下不完整/损坏的工件。
- API/序列化失败会被追加并持久化，但保存后仍会重新抛出（agents/default.py:116-121），因此正常 CLI 会以异常退出，而不是返回干净的失败结果。没有模型流取消。
- 全局调用限制逻辑存在 off-by-one：models/__init__.py:24-30 递增 `_n_calls`，随后测试 `call_limit < _n_calls + 1`；限制为 1 时第一次模型调用就会抛出。测试覆盖启动显示，但没有执行调用限制。
- 完成处理会在 finally 路径中、Submitted 退出消息之前追加填充的未执行观察结果（agents/interactive.py:123-138）；该结果会持久化，可能使最终记录看起来像成功命令没有执行，即使其输出已经成为提交内容。
- 本地主要执行有意使用不受限制的 shell=True，并将继承的主机环境与配置变量合并（environments/local.py:23-29, 71-91）；确认是交互式 UX 策略，而不是沙箱/安全边界。

## 与 Kiana 的映射

- 入口：run/mini.py:53-104 -> config/__init__.py:30-60 -> models/__init__.py:44-61、environments/__init__.py:29-32、agents/__init__.py:24-27。
- 上下文/提示：agents/default.py:51-66, 87-94；config/mini.yaml:1-18, 22-41。
- 推理/解析：models/litellm_model.py:63-105, 127-150；models/utils/actions_toolcall.py:29-112。
- 策略/执行：agents/interactive.py:123-180；environments/local.py:23-55, 71-91。
- 持久化/UI：agents/default.py:158-189；agents/interactive.py:42-55；run/utilities/inspector.py:152-175, 277-284。

## 源码证据

- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/pyproject.toml:88-92 注册 mini、mini-swe-agent 和 mini-extra 入口。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/run/mini.py:53-104 解析入口选项，合并配置，提示缺失的任务，构造模型/环境/agent 并调用 agent.run。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/agents/default.py:87-151 初始化提示消息，执行限制，查询模型，跟踪成本并注入观察结果；:158-189 序列化并写入轨迹 JSON。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/agents/interactive.py:42-55 渲染消息/UI 输出；:123-180 通过确认/白名单策略控制执行；:143-159 处理提交/新任务确认；:183-208 处理交互模式命令。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/models/litellm_model.py:63-105 发送完整消息的非流式完成请求，重试，计算成本，解析工具调用并保存响应元数据；:139-150 格式化工具结果。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/models/utils/actions_toolcall.py:29-75 验证 bash 工具调用；:78-112 创建工具结果消息并填充未执行操作。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/src/minisweagent/environments/local.py:23-55 执行命令并将完成标记转换为 Submitted；:71-91 超时后终止进程组。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/tests/agents/test_default.py:130-253 覆盖完成、限制、超时和部分输出；:333-406 覆盖消息/观察结果跟踪；:480-492 覆盖重复格式错误终止。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/tests/agents/test_interactive.py:132-179 覆盖确认/拒绝恢复；:182-359 覆盖 yolo/帮助/模式切换；:546-591 覆盖中断处理。
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/tests/models/test_actions_toolcall.py:12-143 覆盖解析器错误、有效调用、多调用和观察结果格式；/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/mini-swe-agent/tests/run/test_save.py:9-70 覆盖轨迹 schema 和类元数据。
