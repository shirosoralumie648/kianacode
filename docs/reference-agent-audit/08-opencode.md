# opencode — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/opencode`
- **主要运行时：** opencode 仅源码 Agent 实现审计
- **审计范围：** `packages, core/session, agent, provider, tool, server and TUI`

本报告基于源文件、测试、清单和可执行入口。

## 端到端流程

- 用户在 TUI 中输入提示。需要时提示会创建会话，将 agent/model/variant/parts 提交给类型化 HTTP 客户端，然后立即清除并导航。
- HTTP API 验证会话并调用 `SessionPrompt.prompt`。它通过事件发布写入用户消息，随后 `SessionRunState` 序列化循环；`prompt_async` 不返回内容，失败则变成会话错误事件，而 `prompt` 在完成后返回只含一条消息的 JSON 流。
- `runLoop` 重新加载持久化/事件投影的历史，将其转换为提供商模型消息，解析 agent/model/system 上下文/工具，创建助手记录，然后由 `SessionProcessor` 调用 `LLM.stream`。
- 提供商解析加载已配置的 SDK/模型。AI SDK 的 `streamText` 或可选原生运行时发出规范化的 `LLMEvent` 值。Processor 通过事件持久化流式思考/文本/工具片段；工具调用被解析为待处理/运行中片段。
- AI SDK 调用 `SessionTools` 包装器。每个工具获得 abort/message/call 上下文及合并后的 agent/会话策略。`Permission.ask` 会允许、拒绝，或在 `Deferred` 上阻塞，直到 TUI/服务器回复。工具输出完成后写入工具片段，并转换回模型工具结果内容（包含媒体变通方案）。
- 循环重复，并将工具结果注入历史。普通完成且没有工具调用、结构化输出、内容过滤/错误、取消或压缩/步骤条件会终止循环；压缩会安排另一次循环迭代。
- 会话更新事件被投影到 SQLite 表，并通过过滤后的 SSE 并发发送。TUI 同步消息/片段增量、权限/问题/状态，并渲染最终助手文本/工具记录。重新连接/打开会话时，TUI 从 HTTP 载入数据，并合并正在传输的事件以恢复状态。

## 组件与边界

- 主要运行时：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode（TypeScript/Bun，package.json v1.18.21，bin ./bin/opencode）。次要运行时：packages/core 事件/投影/会话-v2 基础设施；packages/tui Solid/OpenTUI 客户端；packages/cli 守护进程/框架包装器；packages/server 是一个小型导出支持包。未使用被禁止的 README/CLAUDE/AGENTS/USER/docs/changelog/history 内容。

## 状态、持久化与恢复

- 会话状态为 idle/busy/retry/compacting，由 Runner 按会话协调；取消会中断 fibers，并标记助手/工具状态，以便安全重放恢复。
- 工具待处理/运行中片段在重放时转换为 interrupted output-error 模型片段，避免留下悬空的提供商工具使用块。
- 权限请求是限定在一个实例内的内存 `Deferred`；作用域关闭时，finalizer 会拒绝待处理请求。

## 工具、策略与副作用

- Read
- Bash
- multi_tool_use.parallel
- StructuredOutput

## 输出与呈现

- 主要运行时是 packages/opencode；core v2 以及 packages/cli/server 是次要路径。
- 跟踪的具体请求完整覆盖了从 TUI 或 HTTP 入口，到会话创建/恢复、上下文/模型请求、流式事件解析、策略批准、工具执行/结果重新注入、循环/终止、持久化、SSE/UI 输出以及取消/错误。
- 重要设计特征：session/session.ts 以事件优先；数据库持久性由 core SessionProjector 实现，而实时 UI 由同一事件流驱动。
- 默认模型执行使用 AI SDK 工具分派，并规范化为 processor 事件模型；原生运行时为可选项，并带有回退。

## 测试与验证

- 上述引用的源码测试覆盖会话 prompt/processor、提供商加载、权限行为、HTTP 会话操作、投影器持久性/顺序以及 TUI 相关事件契约。
- 未执行测试；这是按要求进行的仅源码审计。

## 优点

- HTTP 入口、提示编排、processor/事件规范化、提供商适配器、工具注册表、策略、持久化投影器和 TUI 同步之间有明确分离。
- 取消/重试/压缩行为丰富，并测试了流中错误、溢出、中止、待处理工具和序列化运行。
- 事件流在连接输出前即主动订阅，TUI 恢复期间明确合并已观察到的事件。
- 工具执行会将结构化元数据、附件、截断信息、快照补丁和用量带入持久化片段/UI。

## 风险与缺口

- 事件优先持久化意味着正确性依赖投影器已挂载并消费事件；会话服务方法本身不会直接写入 MessageTable/PartTable。
- HTTP prompt 的响应是一个序列化的最终消息；渐进式输出由事件/SSE 驱动，因此未订阅的客户端无法观察中间增量。
- TUI 在已载入视图中只保留最近 100 条消息，但服务器/数据库历史仍可通过分页获取。
- AI SDK 负责默认工具分派，而 processor 负责事件持久化和循环控制；特定提供商的规范化是一个需要广泛提供商测试的兼容性边界。

## 与 Kiana 的映射

- 入口：可执行文件 -> packages/opencode/src/index.ts -> HTTP API/TUI 客户端
- 状态：Session + SessionRunState/Runner 序列化每个会话中的一个活动运行
- 推理：Agent 配置 + system/instruction/skills/MCP + MessageV2 转换
- 行动：SessionProcessor 规范化 LLM 事件 -> SessionTools/ToolRegistry -> 具体工具
- 安全：Permission 通配符规则、延迟批准、循环保护、外部目录检查
- 学习/历史：事件发布 -> core SessionProjector SQLite 持久化 -> TUI SSE/载入恢复
- 输出：助手文本/思考/工具片段、HTTP prompt 响应、SSE/TUI 渲染

## 源码证据

- 入口/可执行文件：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/bin/opencode:9-43 生成原生二进制，继承 stdio，转发 SIGINT/SIGTERM/SIGHUP，并转传退出状态；:45-74 选择平台/缓存/环境目标；:189-199 在缺失时报告错误。源码 CLI /media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/index.ts:32-77 解析参数、设置 OPENCODE/AGENT/PID 和 flags；:80-102 注册命令；:117-141 格式化错误并退出。
- HTTP/TUI 入口：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/server/routes/instance/httpapi/groups/session.ts:77-104 定义会话路径，包括 POST /session/:sessionID/message 和 /prompt_async；:202-213 定义创建路径。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts:47-61 解析服务；:154-175 创建会话；:294-329 验证并调用 prompt，或以错误事件派生 prompt_async；:231-234 中止；:362-377 回复权限请求。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/tui/src/component/prompt/index.tsx:930-945 防止重复提交；:947-1024 验证并创建会话；:1059-1111 分派 shell/command/普通提示；:1122-1146 清除输入并导航。
- Prompt/上下文/循环：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/session/prompt.ts:1052-1071 加载会话、清理 revert、创建/保存用户消息、应用工具权限覆盖并进入循环；:1081-1147 加载压缩历史、检测已有工具调用/正常终止、标题生成、模型以及子任务/压缩处理；:1161-1184 检测溢出、解析 agent、最大步骤和提醒；:1186-1201 创建助手消息；:1213-1241 创建 processor 并解析工具；:1257-1286 组装环境/指令/MCP/skills 和 MessageV2 模型消息，然后处理；:1288-1335 处理结构化输出、内容过滤、压缩、停止/继续；:1343-1347 使用 SessionRunState 序列化执行。
- 模型/提供商/流：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/session/message-v2.ts:130-194 和 :197-240 转换用户历史；:243-360 转换助手文本/思考/工具完成/错误/待处理状态；:377-413 将不支持的工具媒体注入为合成用户内容，并调用 convertToModelMessages。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/session/llm.ts:84-112 解析语言模型/配置/认证并准备请求；:223-278 使用 AI SDK 回退的可选原生运行时；:279-352 配置 streamText 的工具、中止、重试、headers 和消息；:357-381 规范化原生或 AI SDK fullStream。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/provider/provider.ts:1847-1869 验证提供商/模型；:1871-1900 解析 SDK 并缓存 LanguageModelV3。
- 工具解析/执行/策略：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/session/processor.ts:97-113 捕获快照/上下文；:122-252 使用 Deferred 跟踪工具调用并更新片段；:277-421 消费思考/工具开始-增量-结束/调用/结果/错误事件并应用循环保护权限；:423-482 记录步骤用量/快照/补丁/压缩；:485-529 流式发送文本增量；:627-681 通过事件处理运行 llm.stream，在压缩时停止，标记中断，重试已识别失败，清理并返回 compact/stop/continue。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/session/tools.ts:40-89 构建工具上下文（abort、消息/调用 ID、元数据和合并后的权限 ask）；:91-133 使用 schema 和插件前后钩子包装注册表工具。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/tool/registry.ts:120-251 加载本地/插件/自定义/内置工具；:264-276 根据权限过滤子代理任务工具。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/permission/index.ts:27-36 评估最后匹配的通配符；:66-106 允许/拒绝/询问延迟请求和 Asked 事件；:108-165 once/reject/always 回复行为。
- 具体工具/取消：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/tool/read.ts:229-262 解析路径、检查外部目录并询问读取权限；:264-374 处理目录、媒体、二进制拒绝、行数限制和输出元数据。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/tool/shell.ts:609-641 解析/扫描命令，询问 external_directory 并执行；:428-557 流式处理子进程输出，截断到文件/元数据，竞争退出/中止/超时并终止进程；:585-594 返回输出/标题/退出元数据。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/session/run-state.ts:34-67 为每个会话存储一个 runner；:70-104 处理忙碌、取消、序列化 prompt 和 shell；:110-142 递归取消子后台任务。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/effect/runner.ts:58-89 将中断映射到取消和 idle；:114-137 合并并发运行；:139-168 处理 shell 忙碌/中断；:170-201 取消所有状态。
- 持久化/恢复/事件/UI：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/session/session.ts:499-538 发出 Created；:540-544 读取 SessionTable；:629-643 发布 updateMessage/updatePart 事件；:667-731 创建/派生并克隆消息和片段。实际持久化是事件投影：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/core/src/session/projector.ts:214-232 插入 SessionTable；:260-272 upsert MessageTable；:310-327 upsert PartTable 和用量。SSE：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/src/server/routes/instance/httpapi/handlers/event.ts:25-41 按实例/工作区主动排队并过滤事件；:59-85 发出 connected、heartbeat 和 disposed 终止事件。TUI：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/tui/src/context/sdk.tsx:81-115 以指数退避重新连接 SSE 并批处理事件；/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/tui/src/context/sync.tsx:175-224 应用权限事件（自动模式回复 once）；:272-358 会话/消息更新；:376-415 片段/增量；:451-551 处理提供商/agent/配置/会话/状态的引导，并区分致命/非致命错误；:594-667 载入会话/消息/todo/diff，同时合并载入期间收到的事件并保留 100 条消息。
- 测试：/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/test/session/processor-effect.test.ts:239-285 验证 LLM 输入/文本/继续；:374-418 溢出压缩；:517-605 重试分类；:711-763 重试状态；:808-872 AI SDK 工具调用；:873-940 待处理工具中止；:941-1014 中止/错误/idle。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/test/session/prompt.test.ts:158-243 构建分层 prompt/HTTP harness；:391 起测试持久化目录和 prompt 行为。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/core/test/session-projector.test.ts:47-78 移动会话；:80-129 revert 持久化；:132-198 持久化顺序。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/test/server/httpapi-session.test.ts:238-260 忙碌映射；:391 起测试持久化目录请求。/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/opencode/test/provider/provider.test.ts:111-176 环境/配置/禁用/白名单；/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/opencode/packages/core/test/permission.test.ts:104-152 允许/拒绝/询问；:231-282 已保存批准和 once。
