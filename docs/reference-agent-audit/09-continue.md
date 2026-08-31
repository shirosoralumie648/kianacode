# continue — 源码补审

- **状态：** `audited`
- **参考路径：** `reference/continue`
- **主要运行时：** CLI/VS Code 共用的 Core + ChatHistoryService + streaming Agent loop
- **审计范围：** `extensions/cli, core, extensions/vscode, GUI, session, stream, permissions, tools`

## 端到端流程

```text
CLI / VS Code / GUI 输入
  → session create/load/resume/fork
  → ChatHistoryService
  → compaction 与 system/tools assembly
  → streamChatResponse
  → provider streaming
  → text/tool-call delta
  → permission policy/approval
  → tool execution
  → tool result 写回历史
  → 自动继续/repair/compact
  → final text/events
```

- CLI 入口和选项：`reference/continue/extensions/cli/src/index.ts:58-218,263-280`；支持 chat、prompt/stdin、print、resume、fork 和 SIGINT。
- Chat 初始化、resume/fork、compact：`extensions/cli/src/commands/chat.ts:88-193`。
- 用户消息先触发自动 compact，再写入 history，随后 stream、title generation、headless output 和成功后的 persistence：`chat.ts:342-405`。
- UI `useChat` 复用同一 ChatHistoryService，管理 AbortController、queued messages、resume 和 interrupted history：`extensions/cli/src/ui/hooks/useChat.ts:76-144,223-411`。

## Session 与历史

- Session 目录、UUID、cwd、history、usage 和立即 persistence：`extensions/cli/src/session.ts:35-178`。
- resume 选最近 JSON session，fork/create 有明确入口：`session.ts:264-357`。
- ChatHistoryService 提供 immutable history、undo/redo、compactionIndex、tool-call state 和 tool result update：`extensions/cli/src/services/ChatHistoryService.ts:23-70,104-204,294-400`。
- `getHistoryForLLM` 按 compaction slice 重建模型上下文：`ChatHistoryService.ts:411+`。

## Prompt 与模型流

- `streamChatResponse` 会校验 context，必要时裁剪末尾消息，再转换为 OpenAI history，使用 backoff 创建 stream：`extensions/cli/src/stream/streamChatResponse.ts:221-291`。
- 读取 chunks、usage/cost、abort、文本和 tool-call delta：`:293-418`。
- 每次循环重新计算 system message、tools 和 permission mode，保证权限变化生效：`:423-456`。
- compaction 后可加入 `continue` user message 自动续跑：`extensions/cli/src/stream/streamChatResponse.helpers.ts:94-127`。

## Tool、Policy、Approval

- `handleToolCalls` 保存 assistant tool calls、创建 states、预处理、执行和保存 tool result：`extensions/cli/src/stream/handleToolCalls.ts:36-170`。
- permission checker 支持 wildcard tool/Bash 规则、参数匹配和 first-match policy：`extensions/cli/src/permissions/permissionChecker.ts:17-181`。
- PermissionManager 维护 pending request、EventEmitter、approve/reject promise 和 permission response：`permissions/permissionManager.ts:10-98`。
- allow/ask/exclude 的结果经过 `executeStreamedToolCalls`；headless ask 会变成 policy denial：`streamChatResponse.helpers.ts:112-137,469+`。
- Terminal command 经过安全策略、preview、timeout、输出流和截断：`core/tools/runTerminalCommand.ts:118-360`。
- writeFile 先生成 diff/new-file preview，再写入文件：`core/tools/writeFile.ts:31-184`。

## 中断、重试与输出

- AbortError 会返回 partial content，不携带未完成 tool calls：`streamChatResponse.ts:362-390`。
- provider retry、context exhaustion、compaction 和 auto continuation 均在 Chat/stream 层处理。
- CLI print mode 输出 JSON/text；VS Code bridge 用 message ID 和 `{done:false,content}` chunk 传递流：`extensions/vscode/src/webviewProtocol.ts:18-115`。
- Core 为每个 message 维护 AbortController：`core/core.ts:89-109,293-295,563-642`。

## 测试覆盖

- streaming/tool loop：`extensions/cli/src/stream/streamChatResponse.test.ts`、`handleToolCalls.test.ts`。
- auto continuation/compaction、TUI stopping、session manager/file operations、permission manager、SDK session 等测试覆盖主要边界。
- Core terminal/tool handlers 位于 `core/tools` 与核心测试。

## 优点

- CLI、VS Code、GUI 共用 Core/ChatHistory/stream loop。
- session resume/fork、compaction、tool states 和 permission pending 状态清晰。
- 流式 tool delta 可组装 JSON 参数；工具结果会进入下一模型请求。
- 终端工具有 timeout、abort、output truncation 和安全检查。
- interrupted path 会保留 partial assistant 内容并支持继续。

## 风险与缺口

- headless 流中断时 partial assistant persistence 需要单独确认；不同 UI 路径可能行为不一致。
- 多工具 rejection 后是否继续后续调用需以测试和 policy 语义严格固定。
- 每次 tool state 更新都写 history，正确性强但可能产生高 I/O。
- permission policy 是静态/动态组合，必须防止 headless 默认 denial 被误解为成功。
- session JSON persistence 仍需 atomic write、revision 和跨进程 ownership 才能成为可靠 Run store。

## 对 Kiana 的映射

- `ChatHistoryService` → Kiana ModelTranscript + event-derived projection。
- `streamChatResponse` → Kiana ModelGateway/normalized stream。
- `PermissionManager` → Kiana ApprovalStore/ApprovalRequirement。
- Core tool handlers → Kiana CapabilityBroker executor。
- CLI/VS Code/Webview → Kiana thin transport adapters。
- compaction/continue/retry → Kiana bounded recovery policy。

Continue 最值得 Kiana 复用的是“一个共享 runtime 支持 CLI/IDE/GUI”，而不是继续维护多套 Agent loop。
