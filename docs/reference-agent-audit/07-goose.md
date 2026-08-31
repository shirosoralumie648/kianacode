# goose — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/goose`
- **主要运行时：** goose
- **审计范围：** `crates, services, ui, extensions, agent loop and tool execution`

本报告基于源文件、测试、清单和可执行入口。

## 端到端流程

- 入口：CLI `main` -> `cli()` -> `build_session` -> 交互式/无头 `process_message`；桌面端/ACP 使用 `session/new`，或先查找持久化会话再调用 `session/prompt`。
- 会话：`SessionManager` 创建/加载持久化会话，其中包含工作目录、对话、提供商/模型、模式、扩展数据、用量和配方；恢复会话时还原扩展配置和模型设置。
- 提示组装：`Agent::reply` 添加 ID、处理引导响应，在启用状态机时分派状态机，否则加载会话/对话、钩子、命令和自动压缩；状态机操作聚合工具 schema、提示片段、MOIM/轮次上下文和系统提示。
- 模型：提供商接收投影/修正/合并后的对代理可见的对话、系统提示、工具/工具垫片工具和模型配置；`Provider::stream` 包装了会话上下文、首项重试以及分块计时/用量。
- 解析：流式助手分块变为持久化助手消息；工具请求会被规范化/强制转换/去重，并分离前端请求与外部请求。
- 策略/批准：`ToolApproval` 检查权限/安全性；批准请求变为仅用户可见的 `ActionRequired` 消息，并修补可执行元数据；CLI/ACP/UI 回答批准请求，答案在下一机器步骤前持久化。
- 执行：`ToolExecution` 通过 `ExtensionManager` 分派已批准调用，应用前置/后置钩子和安全策略，转发 MCP 通知/需要操作的消息，并构造用户角色的工具响应。
- 循环：持久化的工具响应会触发下一次推理步骤；没有待处理工具请求且出现普通助手消息时，操作不再适用，运行结束。最大轮次、压缩、重试、停止钩子和未知工具处理都可能改变或产出该循环。
- 输出/恢复：`Emitter` 流式发送 `AgentEvents`；CLI 渲染事件并更新本地历史；ACP 发送 `SessionNotifications`；副作用持久化对话/用量/扩展状态，恢复过程在每一步重新加载状态。
- 取消/错误：令牌选择会中断提供商/工具流；推理会合成取消用的工具结果，CLI 清理被中断的历史，ACP 返回 `Cancelled`；提供商错误变为错误消息/事件，流创建错误会被呈现。

## 组件与边界

- 主要运行时：Rust CLI 和核心 Agent；次要运行时：ACP 服务器和 Electron 桌面 ACP 客户端。
- CLI 入口与会话构造：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose-cli/src/main.rs:18-52; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose-cli/src/session/builder.rs:492-542, 606-661, 711-918.
- Agent 循环与状态机选择：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/agent.rs:1782-1918, 1921-2033, 2196-2265, 2269-2439; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/mod.rs:0-66.
- 提供商请求/流及工具准备：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/ops_llm.rs:337-610; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/reply_parts.rs:185-229, 295-399, 402-571, 574-743.
- 批准、策略、执行和结果：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/ops_tool_approval.rs:41-153; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/ops_toolcalling.rs:625-677, 797-987, 107-159, 238-397.
- 持久化/恢复/副作用：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/session/session_manager.rs:61-97, 419-480; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/session.rs:20-219.
- CLI 事件/UI/取消：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose-cli/src/session/mod.rs:587-599, 1483-1785, 1788-1865.
- ACP 次要流程：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/acp/server.rs:1694-1735, 1933-2140, 2184-2205; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/ui/desktop/src/acp/chatSessionController.ts:80-230, 281-306; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/ui/desktop/src/acp/prompt.ts:1-20.

## 状态、持久化与恢复

- 仅基于源代码完成审计。
- 已确认主要运行时为 Rust CLI + 核心 Agent；ACP/Electron 列为次要运行时。
- 已跟踪一个具体请求，从 CLI 入口经过会话/模型/上下文、流、工具策略/执行、结果注入、循环终止、持久化、事件/UI 和取消。
- 未修改或创建任何文件。

## 工具、策略与副作用

- find
- grep
- Read
- multi_tool_use.parallel
- StructuredOutput

## 输出与呈现

- 总体：Goose 的主要实现使用 Rust。CLI 和 Agent 旧版循环是生产入口；状态机循环是通过 `GOOSE_STATE_MACHINE=1`（或 bang-shell）选择的可选迁移路径。ACP 是由桌面端和协议客户端使用的一个重要次要运行时。
- 具体请求路径是持久的：入口持久化/加载会话，组装可见对话和动态上下文，流式传输模型输出，验证/规范化工具调用，应用策略/批准，执行扩展，注入工具结果，通过持久化状态循环，并发出最终输出及用量。
- 代码具有异常明确的恢复语义：在可用时恢复提供商会话 ID，副作用在事件发布前应用到存储，状态机运行在每个操作之间重新加载持久化会话。
- 重要架构发现：两个 Agent 循环实现仍处于活动状态。`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/agent.rs:1977-1984` 控制状态机，而旧版循环仍在同一文件中。任何行为变更都必须检查两条路径之间的等价性。
- 未断言仅通过源码发现的缺陷；这是一次跟踪/审计。源码路径为绝对路径，且仅限源代码、测试、清单和可执行入口；未使用被禁止的文档和 git 历史。

## 测试与验证

- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/tests/agent_reply.rs
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/tests/tool_lifecycle.rs
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/tests/compaction_lifecycle.rs
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/tests/hooks_lifecycle.rs
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/tests/agent.rs
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/tests/compaction.rs
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/reply_parts.rs
- /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/acp/server.rs

## 优点

- 明确的持久化会话 schema 包含对话、模型/提供商、模式、扩展、配方、用量以及父会话/会话元数据。
- 状态机副作用将计算与持久化、事件发射分离，支持重建和恢复。
- 提供商流包装器处理投影、角色修正、工具垫片、首项临时重试、计时、用量和错误传播。
- 工具管线在策略检查前规范化格式错误的名称，验证/强制转换参数，去重 ID，并一致地发出前置/后置钩子。
- 测试覆盖生命周期重建、压缩、重试、工具调用、提供商流、ACP 停止原因以及 CLI 恢复/模型选择。

## 风险与缺口

- 双运行时等价性风险：状态机和旧版循环可能在批准、压缩、重试、停止钩子、工具处理和取消方面出现分歧。
- 提供商拥有上下文的风险：无状态对话持久化与提供商恢复/交接是有意不同的；对于拥有上下文的提供商，恢复时更改提供商/模型受到限制。
- 取消可能留下部分流式助手/工具状态；既需要状态机推理取消，也需要 CLI 被中断消息清理。
- ACP 与 CLI 具有不同的批准/引导交互体验和非交互安全策略；应同时测试协议路径和终端路径。
- 扩展加载是并行的，CLI builder 中的失败会变成警告/继续；不可用扩展会改变模型的工具表面。
- 工具调用解析对安全性敏感：必须继续在检查/钩子之前进行规范化，以防策略绕过。

## 与 Kiana 的映射

- Agent 循环：Agent::reply/reply_impl/reply_internal 加上可选的状态机管线。
- 会话/线程：SessionManager 和 ACP 会话 ID 映射。
- 提示/上下文：PromptManager、reply_parts 投影、扩展提示片段和轮次上下文。
- 模型/流：Provider trait 通过 stream_response_from_provider 及各提供商的流实现。
- 工具：ExtensionManager 分派、MCP 客户端/工具执行、前端工具。
- 策略：PermissionManager、ToolInspectionManager、PreToolUse 钩子、ToolApprovalOperation。
- 持久化：SessionManager 存储和状态机 EffectHandler。
- 事件/UI：AgentEvent、Emitter、CLI 输出、ACP SessionNotification、Electron ACP 适配器。
- 恢复/取消：按步骤持久化重载、提供商恢复 ID、CancellationToken 和中断清理。

## 源码证据

- `summarize this directory` 这样的具体请求进入 CLI `process_message`，保存在 `CliSession` 中，并带着 `SessionConfig` 和 `CancellationToken` 到达 Agent::reply（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose-cli/src/session/mod.rs:587-599, 1483-1517）。
- 新会话按 ID 创建或恢复；恢复会选择持久化用户会话，还原提供商/模型和扩展状态，而拥有上下文的提供商会拒绝提供商/模型变更（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose-cli/src/session/builder.rs:316-490, 492-542, 613-624, 724-755）。
- 状态机回复持久化传入的用户消息，解析持久化模型配置/上下文，构建有序操作，并在 `run_goose` 执行期间复用发射器事件（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/agent.rs:1782-1884）。
- 推理构建排序后的/对可见工具过滤后的工具、前端指令、扩展提示、Goose 系统提示和轮次上下文，然后调用提供商流；分块接收推理元数据，重复工具 ID 被丢弃，思考被规范化，用量/错误被转换为副作用（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/ops_llm.rs:370-606）。
- 提供商流投影修正角色，移除隐藏内容和未回答的历史工具请求，支持工具垫片；对于无状态提供商，仅在首项之前重试临时失败，并发出分块/用量元组（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/reply_parts.rs:330-571）。
- 工具调用解析在检查前规范化被破坏的名称，强制转换 schema 参数，合并元数据，去重 ID，并分离前端/外部/扩展调用（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/reply_parts.rs:574-743）。
- 批准会检查待处理请求并标记 `goose.executable`；执行会运行阻塞式 PreToolUse 策略钩子，在带取消能力的情况下通过 ExtensionManager 分派，包装工具后置钩子，流式发送通知/需要操作的事件，并为成功、拒绝、解析错误或中断注入工具响应（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/ops_tool_approval.rs:41-153; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/ops_toolcalling.rs:797-987）。
- 每个状态机副作用都会在相应的用量/历史事件发出前持久化；运行会在每一步前重新加载会话，因此工具结果会供下一次推理使用；当没有操作适用或某操作让出给客户端时终止（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/session.rs:28-135, 153-219; /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose-agent/src/machine.rs:61-151）。
- CLI 渲染文本/JSON/流式 JSON、MCP 进度/日志通知、用量、批准和错误；Ctrl-C 取消令牌，中断清理会添加工具错误或恢复继续提示（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose-cli/src/session/mod.rs:1529-1705, 1788-1865）。
- ACP 将会话 ID 映射到线程，保护一个活动运行，转发 AgentEvent 消息/工具通知/用量到 SessionNotification，返回 EndTurn/MaxTokens/Cancelled，取消时调用活动令牌（/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/acp/server.rs:1933-2140, 2184-2205）。
- 测试覆盖两个运行时和边界情况：状态机生命周期测试位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/state_machine/tests/`；旧版 agent/tool/compaction 测试位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/tests/agent.rs` 和 `compaction.rs`；流重试/投影测试位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/agents/reply_parts.rs`；ACP 测试位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/goose/crates/goose/src/acp/server.rs`。
