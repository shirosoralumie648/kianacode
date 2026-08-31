# claude-code-rust — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/claude-code-rust`
- **主要运行时：** claude-code-rust 仅源码审计
- **审计范围：** `src/main.rs, src/cli, src/session, src/tools, src/services, src/web`

本报告基于源文件、测试、清单和可执行入口。

## 端到端流程

- 具体请求：用户启动 `claude-code`，接受默认 REPL，输入 `read the contents of src/main.rs`。`main` 加载配置/状态，CLI 将流程分派给 `Repl::start`。
- `Repl::new` 创建空的 `conversation_history` 和 MCP 内置工具。`process_input` 检查 API 密钥，追加用户 ChatMessage，并声明 MCP 工具。
- 客户端将此前的所有消息和工具作为一个阻塞式、非流式的 OpenAI 形状请求提交。模型响应要么作为最终文本打印，要么被解析为工具调用。
- 对于 `file_read` 调用，MCP 期待 `{path: ...}`；它读取该路径并返回 JSON。结果作为以调用 id 为键的 role=tool 消息注入，然后再次发起模型请求。当不再有 tool_calls 时，助手内容会被打印，并且只存储在内存向量中。
- 没有会话/线程创建或恢复，没有审批检查点，没有超出此前轮次/工具之外的上下文组装，没有持久化转录，也没有取消路径。API 失败会返回 REPL；工具失败会变成 JSON/错误字符串，循环继续。

## 组件与边界

- 主要运行时：位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/claude-code-rust/src/main.rs` 的 Rust Cargo 二进制 `claude-code`（Cargo.toml 第 126-128 行）。
- 入口/UI：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/claude-code-rust/src/cli/args.rs` 和 `src/cli/repl.rs`；API：`src/api/mod.rs`；REPL 实际使用的工具注册表：`src/mcp/tools.rs`；输出：`src/cli/ui.rs`。
- 次要运行时：GUI `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/claude-code-rust/src/gui/main.rs` 和 Web 插件市场 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/claude-code-rust/src/web/main.rs`；恢复的 TypeScript 树由 `claude-code-rev-main/src/bootstrap-entry.ts` 单独启动。

## 状态、持久化与恢复

- 仅审计了源码、测试、清单和可执行入口。
- 未打开 README.md、CLAUDE.md、AGENTS.md、USER.md、docs 目录、变更日志和 git 历史。
- 未修改任何文件。

## 工具、策略与副作用

- 仅使用 Read、Bash find/grep 进行源码导航和分析。
- 未执行写入/编辑/删除命令。

## 输出与呈现

- 主要最终输出由 `print_claude_message` 在终端渲染，响应使用量中的 token 数来自：`src/cli/repl.rs:230-252` 和 `src/cli/ui.rs:96-137`。
- 单次 `query` 只打印 `choices[0].message.content`，不暴露工具/历史：`src/cli/args.rs:96-149`。
- GUI 最终输出是 egui 占位消息中累积的 SSE 内容：`src/gui/app.rs:268-282`。

## 测试与验证

- 检查了 `tests/integration_test.rs`、`tests/tools_test.rs` 和 `tests/skills_test.rs`；它们覆盖构造、注册表成员关系、简单任务/笔记操作和技能。
- API 传输、流式分块、REPL 工具循环行为、格式错误的工具调用、审批、持久化、恢复、取消和恢复处理均没有生命周期覆盖。

## 优势

- CLI 分派和终端反馈路径清晰。
- 工具调用循环正确保留助手 tool_calls，并按调用 id 注入 role=tool 消息。
- MCP 执行器接口和内置工具模式直观且异步。
- 基础注册表和技能测试为工具注册提供了冒烟覆盖。

## 风险与缺口

- 高：默认 Anthropic base URL 与 OpenAI 兼容的 chat-completions 负载/路径配对，因此默认部署可能无法通过协议验证。
- 高：shell/file write/edit 工具无需用户审批、沙箱、路径策略或项目根目录限制即可执行；模型可以调用任意 `sh -c` 并写入任意路径。
- 高：主要聊天转录仅存在于进程本地且从未保存；重启会丢失上下文，现有会话管理器也无法恢复它。
- 中：主要请求在异步运行时中仍为阻塞式/非流式；不存在取消/信号传播。
- 中：格式错误的工具参数会被静默替换为 `{}`，未知/缺失工具会作为错误字符串返回，而不是受治理的失败。
- 中：`ApiClient::chat_stream` 返回响应时不检查 HTTP 状态，且流式类型省略了工具调用增量解析。
- 中：两个不兼容的 ToolRegistry 实现和两个 SessionManager 实现造成运行时/测试分歧；REPL 使用 MCP 变体，而集成测试面向 `crate::tools`。
- 低：Web 服务器是带硬编码数据和宽松 CORS 的示例插件市场，而不是 agent 入口。

## 映射到 Kiana

- 入口：Rust `main` -> `Cli.run_async` -> `Repl.start/process_input`。
- 上下文：仅有 `conversation_history` 加 MCP 工具定义；`AppState`、memory 和会话管理器未连接到 REPL。
- 模型：`ApiClient` 发布到 `/v1/chat/completions`；主 CLI 不进行流式处理。
- 工具：MCP `ToolRegistry` 和内置执行器；独立的 `crate::tools` 注册表由测试/其他代码使用，而非 REPL。
- 策略：REPL 中没有审批/策略执行；MCP sampling 审批是存根。
- 持久化：配置会持久化；聊天/会话转录不会。
- 输出：终端 UI；GUI 是次要的仅 SSE 客户端；Web 仅为市场。

## 源码证据

- 启动过程加载设置并创建 AppState，然后分派 Cli.run_async：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/claude-code-rust/src/main.rs:8-27`。默认/无子命令进入 REPL，而 `query` 走独立的一次性路径：`src/cli/args.rs:20-25,69-71,96-149`。
- REPL 构造空的进程本地历史并注册 MCP 内置工具：`src/cli/repl.rs:12-35`；读取输入并追加用户消息：`src/cli/repl.rs:37-98`。
- 每一轮只组装克隆的会话历史加工具定义，然后向 `/v1/chat/completions` 发布 `stream:false`：`src/cli/repl.rs:99-123`。该路径中没有系统提示、项目上下文、记忆检索或会话恢复。
- 工具调用解析要求 id/type/name/字符串参数；格式错误的 JSON 会静默变成 `{}`；助手工具调用和 role=tool 结果消息会在循环前追加：`src/cli/repl.rs:148-227`。最终文本被渲染并追加，然后循环退出：`src/cli/repl.rs:230-259`。
- REPL 注册表是 MCP 注册表，而不是 `crate::tools::ToolRegistry`：`src/cli/repl.rs:4,7,15-28`；注册和执行是异步 map 查找：`src/mcp/tools.rs:39-82,84-147`。内置 shell 执行运行 `sh -c` 并返回 stdout/stderr/退出码：`src/mcp/tools.rs:199-257`；文件读/写和搜索位于 `src/mcp/tools.rs:156-197,260-317`。
- API 请求模型、密钥查找、模型映射、超时和非流式请求位于 `src/api/mod.rs:13-107` 和 `src/config/api_config.rs:21-59`。默认 base URL 是 `https://api.anthropic.com`，而请求使用 OpenAI 兼容的 `/v1/chat/completions`：`src/config/api_config.rs:21-34`、`src/cli/repl.rs:109-129`。
- UI 打字效果、助手渲染和请求错误是同步终端输出：`src/cli/ui.rs:96-137,184-203`；REPL 请求错误返回时不会持久化错误事件：`src/cli/repl.rs:125-143`。
- `src/api/mod.rs:77-107` 存在流式 API，但主 CLI 路径强制使用 `stream:false`：`src/cli/repl.rs:109-115` 和 `src/cli/args.rs:114-120`。只有 GUI 消费 SSE 并累积内容：`src/gui/app.rs:191-265`；其 `StreamChunk` 类型只有内容，没有工具调用增量：`src/api/mod.rs:241-260`。
- 没有审批/策略门保护 REPL 工具执行：直接分派位于 `src/cli/repl.rs:200-221`，MCP 执行位于 `src/mcp/tools.rs:77-82`；MCP sampling 只返回占位的“等待审批”响应：`src/mcp/server.rs:184-188`。主要请求路径中不存在取消 token、信号处理或中断传播。
- 持久化管理器存在但未连接。同步 `SessionManager` 在 `~/.claude-code/sessions` 下创建/加载/保存 JSON 会话：`src/session/mod.rs:7-101`；异步 memory session manager 是带活动会话的独立实现：`src/memory/session.rs:83-228`。main/CLI/REPL 都没有调用任一实现。Agent 子命令只创建内存中的 `AgentSession`，发送一次 system+user，且没有工具循环：`src/services/agents.rs:234-306`；取消只改变状态，无法停止正在进行的工作：`src/services/agents.rs:319-328`。
- memory 的加载/保存只通过 memory 命令暴露，而不通过聊天：`src/memory/mod.rs:212-222` 和 `src/cli/args.rs:274-312`。会话写入直接覆盖 JSON，没有原子临时文件/重命名恢复：`src/session/mod.rs:80-88` 和 `src/memory/session.rs:127-134`。
- 测试覆盖注册表存在性/基础工具调用/技能，但不覆盖请求生命周期：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/claude-code-rust/tests/integration_test.rs:11-125`、`tests/tools_test.rs:4-94`、`tests/skills_test.rs:5-133`。没有 API、REPL 循环、工具调用解析、审批、持久化/恢复、流式处理、取消或恢复测试。
