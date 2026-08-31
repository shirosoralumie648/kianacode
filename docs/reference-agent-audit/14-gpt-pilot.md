# gpt-pilot — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/gpt-pilot`
- **主要运行时：** 对 gpt-pilot 参考实现进行仅基于源码的架构与执行审计
- **审计范围：** `core, cloud, prompt loop, coding steps, tools and state`

本报告基于源文件、测试、清单文件和可执行入口。

## 端到端流程

- 入口：`main.py` -> `run_pythagora()` -> `core.cli.helpers.init()` -> `async_main()`。主要 UI 可配置为 PlainConsoleUI、IPCClientUI 或 VirtualUI；本地 API 服务器可运行在端口 8222。
- 会话创建/恢复：新请求会创建项目、分支、初始状态和数据库会话；继续执行时按项目/分支/步骤/状态 ID 加载，并创建 `next_state`。现有文件会被恢复，或在用户作出决定后导入离线变更。
- 提示循环：每个 agent 都会使用系统提示和模板上下文构造 AgentConvo；模型按 agent 名称/提供商配置选择。流式内容推送到 UI，同时保留完整响应，以便解析器/模型后续处理。
- 策略：任务拆分和 shell 步骤通常需要用户批准；可选的拆分审查需要用户再次交互。`INPUT_REQUIRED` 会路由到 HumanInput/编辑器审批。启动依赖项/调试器的安装是自动的，不受命令步骤审批路径控制。
- 动作/结果循环：模型 JSON 变为状态步骤；CodeMonkey 写入文件，Executor 运行 shell 进程。命令输出实时流式传递，同时注入独立的 `ran_command` 分析提示。`DONE` 响应会提交状态、导入磁盘变更并启动由下一状态驱动的 agent；`EXIT` 会跳出编排器循环。
- 终止/错误：任务完成最终会路由到 TechLead 的功能/完成提示，该提示可以返回 EXIT。API 错误会重试，也可能询问用户；运行级异常会向 UI 发送致命输出并调用回滚。UI 关闭映射为 UIClosedError；显式 `interrupt` 映射为 UserInterruptError。

## 组件与边界

- 主要运行时：Python 包 `core`，由 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/main.py:4-29` 启动，并作为 Poetry Python 项目配置/部署（`pyproject.toml:0-41`）。
- 入口/会话：CLI 参数解析、配置/UI 适配器选择、迁移和 `SessionManager` 设置位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/cli/helpers.py:179-280,926-952`；新项目或继续执行的分派位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/cli/main.py:193-320`。
- 编排：状态驱动的 `Orchestrator` 循环和 agent 路由位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/agents/orchestrator.py:35-166,462-572`。
- 提示/模型：基于 Jinja 的 `AgentConvo` 位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/agents/convo.py:19-127`；提供商选择位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/config/__init__.py:324-440`；流式/重试/解析器逻辑位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/llm/base.py:123-412`，OpenAI 传输位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/llm/openai_client.py:17-86`。
- 编码/动作：规划和结构化任务步骤位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/agents/tech_lead.py:295-367` 与 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/agents/developer.py:203-310`；文件和命令执行位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/agents/code_monkey.py:67-157,212-231` 与 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/agents/executor.py:71-166`。
- 持久化/恢复：近似不可变的状态链和 VFS 位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/state/state_manager.py:271-414,416-488,588-759` 与 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/db/models/project_state.py:58-129,268-314,316-425`。
- UI/次级入口：控制台 UI `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/ui/console.py:10-130`；VS Code IPC 客户端 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/ui/ipc_client.py:129-239,315-393`；本地 API/聊天服务器 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/core/ui/api_server.py:42-180,245-380,639-659`。
- 次级 cloud 路径不是 agent 运行时：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/cloud/entrypoint.sh:23-33` 只准备数据库目录、启动安装器并无限 tail。

## 状态、持久化与恢复

- ProjectState 是一个链式快照，其中包含 `prev_state_id`、步骤索引、JSON epics/tasks/steps/iterations、文件关系和首个未完成项选择器（`core/db/models/project_state.py:58-129`）。
- `create_next_state` 深拷贝工作流 JSON，并克隆文件记录/内容引用（`core/db/models/project_state.py:268-314`）。`StateManager.commit` 提交、轮换会话、将 next 提升为 current、再创建一个 next 状态并重新加载文件内容（`core/state/state_manager.py:432-472`）。
- LLM 请求、用户输入和 ExecLogs 都关联当前状态；除非配置启用，否则不会持久化 LLM 请求数据库记录，但始终会记录遥测（`core/state/state_manager.py:490-550`）。
- 离线协调会将 VFS 与当前数据库文件进行比较；用户可以将变更导入 next state，或恢复已存储文件（`core/agents/orchestrator.py:369-415`；`/core/state/state_manager.py:702-759`）。
- Chat API 根据项目状态和客户端提供的历史构造提示，通过切片限制历史长度，流式传输响应，并创建 ChatConvo/ChatMessage 记录（`core/ui/api_server.py:264-350`；`/core/db/models/chat_convo.py:15-48`）。

## 工具、策略与副作用

- 原生模型工具调用：在检查的源码中不存在；`Convo.function` 仅是角色辅助方法，而 Anthropic 会拒绝 function 消息（`core/llm/convo.py:45-115`；`core/llm/anthropic_client.py:40-65`）。
- 结构化动作工具：`TaskSteps` 区分 `save_file`、`command`、`human_intervention` 和 `utility_function`（`core/agents/developer.py:31-80`）。
- 文件工具：CodeMonkey 加载状态文件内容，调用 LLM，通过 StateManager/VFS 写入，发出差异/状态，并将需要输入的文件路由到 HumanInput（`core/agents/code_monkey.py:80-157,212-231`）。
- 命令工具：Executor 请求审批，使用 ProcessManager shell 执行/输出回调，运行分析 LLM，记录 ExecLog，并将命令步骤标记为完成（`core/agents/executor.py:71-166`；`core/proc/process_manager.py:139-265`）。
- 人工/工具结果反馈：HumanInput 打开编辑器并等待；命令输出先流式传递，然后提供给 `ran_command` 提示，而后续迭代依赖持久化的 ProjectState 步骤，而不是追加通用的工具结果消息（`core/agents/human_input.py:28-40`；`core/agents/executor.py:146-166`）。

## 输出与呈现

- 整体设计是状态机式 agent 循环，结合持久化项目快照、模板驱动提示、提供商抽象、结构化 Pydantic 输出、人工审批点和 UI 事件投影。
- 主要运行时是本地 Python CLI/core 包。`cloud` 目录是次级容器/引导程序，不是第二套 agent 实现。
- 具体的 `Build a todo app` 跟踪通过状态提交和 UI 流达到最终输出；编排器不会单独持久化一个最终文本答案——持久化结果是项目状态/文件以及发出的 UI 事件。聊天 API 路径执行时，响应会流式传输并随后存储为 ChatMessage 行（`core/ui/api_server.py:245-350`）。

## 测试与验证

- 清单声明的测试套件使用 pytest/pytest-asyncio，并对 `core` 进行覆盖率测试（`pyproject.toml:43-57`）。
- LLM 测试覆盖 OpenAI 流式传输、请求载荷、解析器重试、重试耗尽、错误处理器重试和速率限制解析（`tests/llm/test_openai.py:19-283`）；解析器测试覆盖代码块、可选块、JSON schema、验证和枚举（`tests/llm/test_parser.py:9-205`）。
- Convo 和 BaseAgent 测试覆盖克隆、消息角色、schema 提示、UI 流回调、LLM 工厂接线和用户输入日志（`tests/llm/test_convo.py:5-295`；`tests/agents/test_convo.py:7-43`；`tests/agents/test_base.py:13-91`）。
- Process 测试覆盖 stdout/stderr 捕获；状态测试覆盖提交链、项目加载、VFS 持久化/导入/恢复（`tests/proc/test_process_manager.py:58-80`；`tests/state/test_state_manager.py:9-225`）。
- IPC 测试覆盖出站消息/流、连接失败、UI 关闭、按钮/默认值映射和无效响应，但伪服务器发送的是未分帧响应，未针对 API 服务器的分帧响应路径进行测试（`tests/ui/test_ipc_client.py:17-68,84-302`）。
- Agent 测试覆盖离线变更导入/恢复以及少量 TechLead 规划场景，但没有从入口经过 Orchestrator -> Developer -> CodeMonkey/Executor -> 提交 -> 最终 UI 输出的端到端测试，也没有直接测试 CodeMonkey 或 Executor 的失败语义（`tests/agents/test_orchestrator.py:7-99`；`tests/agents/test_tech_lead.py:10-121`）。

## 优势

- 清晰的状态驱动路由使每个编码步骤可恢复，并暴露明确的 action/status 字段（`core/agents/orchestrator.py:462-572`；`core/db/models/project_state.py:110-189`）。
- 提供商抽象集中处理流式传输、重试、速率限制、解析器纠正、令牌统计和请求日志（`core/llm/base.py:123-212,250-412`；`core/llm/openai_client.py:32-86`）。
- 提示模板与 agent 逻辑隔离，并使用严格的 Jinja 未定义处理；提示日志保留模板/上下文来源（`core/llm/prompt.py:11-45`；`core/agents/convo.py:63-80`）。
- 结构化 Pydantic schema 约束任务计划、命令分析、文件描述和步骤联合体；解析器测试覆盖格式错误的 JSON 和验证（`core/llm/parser.py:135-195`；`/tests/llm/test_parser.py:71-145`）。
- 面向用户的进度信息丰富：UIBase/IPC 表示流式内容、任务/步骤进度、文件状态、差异、日志、致命事件和明确的输入请求（`core/ui/base.py:122-175,284-471`；`core/ui/ipc_client.py:411-675`）。
- 状态/文件测试验证初始项目创建、按项目/分支/步骤继续、提交、VFS 保存、变更/删除文件导入及恢复（`tests/state/test_state_manager.py:9-225`；`tests/agents/test_orchestrator.py:7-99`）。

## 风险与缺口

- HIGH：IPC 接收分帧似乎不一致。本地 API 服务器用 4 字节长度作为响应前缀（`core/ui/api_server.py:168-180`），而 `IPCClientUI._receive` 读取任意字节，并将包含前缀的缓冲区直接传给 `Message.from_bytes`（`core/ui/ipc_client.py:173-200`）。IPC 测试中的伪服务器写入未分帧 JSON（`tests/ui/test_ipc_client.py:38-68`），因此没有针对服务器实现测试这一不匹配。
- HIGH：Executor 的失败分支被 `if True or llm_response.success` 禁用；即使命令状态非零或 LLM `success=False`，仍会返回 DONE 并推进状态，而不是产生 ErrorHandler 输入（`core/agents/executor.py:108-144`）。
- HIGH：用户取消的传播不一致。Console 和 IPC 将 `interrupt` 映射为 `UserInterruptError`（`core/ui/console.py:114-122`；`core/ui/ipc_client.py:371-376`），但顶层 `run_project` 捕获的是 KeyboardInterrupt/UIClosedError，而不是 UserInterruptError（`core/cli/main.py:81-103`）；只有狭窄的 `agent.run()` 调用捕获 UserInterruptError（`core/agents/orchestrator.py:140-146`）。设置、离线协调或其他 agent 相邻路径中的中断可能变成致命错误。
- HIGH：认证/令牌状态处理在异步 LLM 执行内部调用 `sys.exit(0)`（`core/llm/base.py:299-344`），这可能绕过 `async_main`/`run_project` 中正常的 `finally` 清理、遥测发送、API 服务器停止和状态回滚（`core/cli/main.py:380-425`）。
- MEDIUM：磁盘写入立即发生，而数据库状态延后。在 API 错误、用户中断或异常时，`StateManager.rollback()` 不会恢复 VFS；修改后的文件仍留在磁盘上，只会在下次启动时协调（`core/state/state_manager.py:479-488,588-625`；`/core/agents/orchestrator.py:369-415`）。
- MEDIUM：从历史项目状态恢复具有破坏性：`load_project` 调用 `state.delete_after()` 并提交，在继续执行前删除所有更晚的状态（`core/state/state_manager.py:322-350`）。
- MEDIUM：启动依赖项设置绕过用户命令审批并自动运行 shell 命令（`core/agents/orchestrator.py:168-180,221-245`），而 `ProcessManager` 使用 `create_subprocess_shell`（`core/proc/process_manager.py:46-55`）。
- MEDIUM：本地 VFS 会规范化路径，但不强制路径位于项目根目录下（`core/disk/vfs.py:137-145`）；`StateManager.save_file` 接受模型提供的路径（`core/state/state_manager.py:588-625`），因此生成的 `../` 路径可以逃逸工作区。
- MEDIUM：`ChatConvo` 路由是服务器全局的：START_CHAT 分配 `self.convo_id`，历史写入使用该字段，而不是每个请求/客户端的 ID（`core/ui/api_server.py:201-240,366-380`）。并发聊天客户端可能发生碰撞，或将消息写入错误的会话。
- LOW：基础 `_adapt_messages` 实现将会话条目当作对象（`msg.role`、`msg.content`），但 `Convo` 存储的是字典；Anthropic 覆盖实现是正确的，因此对于使用基础实现的任何提供商而言，该实现处于潜在损坏状态（`core/llm/base.py:97-121`；`/core/llm/anthropic_client.py:40-65`）。
- LOW：没有编排器端到端取消/恢复测试，也没有原生工具调用测试；当前测试验证的是相互隔离的提供商/解析器/UI/进程/状态行为。

## 映射到 Kiana

- 用户请求 -> CLI/API 入口 -> StateManager 项目/分支状态
- 提示/上下文 -> AgentConvo Jinja 模板 + 序列化状态/相关文件
- 模型 -> BaseLLMClient 提供商适配器 + 流式 OpenAI/Anthropic 兼容请求
- 工具等价物 -> 区分型 TaskSteps（`save_file`、`command`、`human_intervention`、`utility_function`），而不是原生模型工具
- 持久化 -> SQLAlchemy ProjectState 链、File/FileContent 记录、可选 LLM/用户/exec 日志、VFS
- 事件/UI -> UIBase 适配器、分帧 IPC 消息、控制台输出、项目/任务/文件进度
- 恢复 -> 提交/回滚、离线导入/恢复、历史状态加载/截断

## 源码证据

- 具体请求跟踪（新项目，用户说 `Build a todo app`）：`main.py` 导入并通过 `run_pythagora` 退出（`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/gpt-pilot/main.py:4-29`）；`init` 解析 `--initial-prompt`、选择 UI、运行迁移并创建数据库管理器（`core/cli/helpers.py:926-952`）。`async_main` 创建 `StateManager`，可选启动 API 服务器，启动 UI，安装信号/atexit 清理，并调用 `run_pythagora_session`（`core/cli/main.py:323-425`）。
- 对于新请求，`start_new_project` 在存在 `args.initial_prompt` 时选择 Node 技术栈并调用 `StateManager.create_project`；否则请求选择技术栈（`core/cli/main.py:120-190`）。初始状态包含空的 epics/specification 和知识库（`core/db/models/project_state.py:214-234`）。
- 第一次编排器迭代初始化 `Executor`，执行离线变更协调、依赖项/调试器设置，然后由于没有 epics 创建 `Wizard`（`core/agents/orchestrator.py:47-86,480-484`）。`Wizard` 创建前端 epic 并返回 `CREATE_SPECIFICATION`（`core/agents/wizard.py:23-29,112-130`）；随后由状态驱动的迭代路由到 `SpecWriter`，它使用 CLI 提示或询问 UI，流式传输完整规格提示，为项目命名，初始化文件系统，并写入 specification/knowledge-base 字段（`core/agents/spec_writer.py:62-160`）。
- 每次 `DONE` 后，`Orchestrator.handle_done` 提交 next state、导入外部变更，并可能路由到文件描述或人工输入（`core/agents/orchestrator.py:417-460`）。循环随后经过前端/架构师/技术负责人规划；TechLead 请求模型生成 JSON 开发计划，并将子 epic 转换为任务（`core/agents/tech_lead.py:295-367`）。
- 对于一个生成的后端任务，Developer 并行获取相关文件；除非任务标志跳过审批，否则请求任务审批；流式传输拆分提示；对格式错误的代码标签输出最多重试两次；可选执行人工拆分审查；然后请求 JSON 解析器生成 `TaskSteps` 并存储未完成步骤记录（`core/agents/developer.py:94-118,203-310`；相关文件上下文 `/core/agents/mixins.py:121-187`）。
- `TaskSteps` 是 `save_file`、`command`、`human_intervention` 和 `utility_function` 的区分型联合体（`core/agents/developer.py:31-80`）。Orchestrator 按类型路由第一个未完成步骤（`core/agents/orchestrator.py:508-572`），而 `ProjectState.current_step` 始终选择第一个未完成步骤（`core/db/models/project_state.py:110-129`）。
- 对于 `save_file`，CodeMonkey 加载旧内容，渲染实现上下文，调用配置的模型，去除可选代码块，发出文件状态/差异，通过 `StateManager.save_file` 写入，将步骤标记为完成，并在哨兵行要求人工编辑时返回 `INPUT_REQUIRED`（`core/agents/code_monkey.py:80-157,212-231`；`/core/llm/parser.py:122-133`）。下一次编排器迭代将该响应路由到 HumanInput，后者打开文件并等待编辑器响应（`core/agents/human_input.py:9-40`）。
- 对于 `command`，Executor 请求是/否/编辑审批，通过 ProcessManager 运行 shell 命令，将 stdout/stderr 流式传给 UI，然后把命令输出和退出状态提交给独立的 JSON `CommandResult` LLM 提示（`core/agents/executor.py:71-166`；`/core/proc/process_manager.py:217-265`）。它记录命令，将步骤标记为完成，编排器随后提交并循环（`core/state/state_manager.py:538-550`）。
- LLM 请求会派生会话，近似统计令牌数，将超过 150k 令牌的旧日志密集型消息裁剪掉，流式传输提供商输出，并重试传输/解析器失败；解析器错误会作为 assistant 输出注入，并附带用户纠正请求（`core/llm/base.py:158-212,250-412`）。OpenAI 使用 `chat.completions.create(stream=True)`，将块转发给流处理器，发送结束标记，并在使用量缺失时估算使用量（`core/llm/openai_client.py:32-86`）。
- 提示/上下文组装从系统提示和 Jinja 模板路径 `<agent_type>/<name>.prompt` 开始；默认变量暴露完整当前状态和 OS，提示日志序列化模板上下文（`core/agents/convo.py:19-80`）。结构化 JSON schema 通过 `require_schema` 追加，`JSONParser` 验证 Pydantic 输出并保留原始文本（`core/agents/convo.py:111-127`；`/core/llm/parser.py:135-195`）。
- 检查的源码中没有通用/原生工具调用注册表或工具调用解析器。Convo 中存在 function 角色，但 Anthropic 会拒绝它；操作动作是模型生成的区分型步骤以及直接的 shell/VFS 调用（`core/llm/convo.py:45-115`；`/core/llm/anthropic_client.py:40-65`；`/core/agents/developer.py:31-80`）。
- UI 输出使用 `BaseAgent.stream_handler` 发送带有 agent/state/route 元数据的块（`core/agents/base.py:125-139`），OpenAI/Anthropic 发送最终的 `None` 块（`core/llm/openai_client.py:67-76`；`core/llm/anthropic_client.py:91-111`），IPC 将其映射为 `<__stream_end__>`（`core/ui/ipc_client.py:218-239`）。文件状态、差异、进度、问题、致命错误和日志由 IPC 适配器单独发出（`core/ui/ipc_client.py:315-393,411-475,564-675`）。
- 状态恢复与磁盘不是事务性的：`save_file` 立即写入 VFS，但只将数据库记录附加到 `next_state`；`rollback` 会回滚/关闭数据库会话而不恢复文件（`core/state/state_manager.py:479-488,588-625`）。加载较旧状态会调用 `delete_after` 并提交，然后继续执行，从而截断分支中更晚的状态（`core/state/state_manager.py:322-350`）。
