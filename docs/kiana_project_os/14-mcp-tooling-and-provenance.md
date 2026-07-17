# Volume 14: MCP、工具与 Provenance

## 1. 目标

MCP 和工具系统让 Kiana 接外部世界，但也带来风险。Kiana 需要把工具可见性、来源、权限、启动状态和失败策略做成一等对象。

## 2. 工具分类

| 类型 | 示例 | 风险 |
| --- | --- | --- |
| local read | read file, search | low/medium |
| local write | edit file, patch | medium/high |
| shell | test/build | medium/high |
| network | docs/API | high |
| browser | web interaction | high |
| MCP tool | external server | variable |
| EDA tool | BOM/Gerber checker | medium/high |

## 3. Tool Registry

字段：

- tool_id。
- name。
- provider。
- type。
- capabilities。
- input schema。
- output schema。
- permission requirement。
- visibility。
- trust level。

Tool Registry 不是单纯列表，它参与 router 和 policy。

## 4. MCP Server Registry

字段：

- server_id。
- name。
- source。
- command/url。
- transport。
- required。
- startup_status。
- trust_level。
- exposed_tools。
- exposed_resources。
- exposed_prompts。

## 5. Provenance

Provenance 记录：

- 谁安装。
- 从哪里来。
- 什么版本。
- hash/signature。
- 哪个 policy 允许。
- 哪些工具暴露给哪些 mode。

没有 provenance 的 MCP 不应默认启用。

## 6. Visibility

可见性层级：

- hidden。
- listed。
- callable。
- callable_with_approval。
- blocked。

Visibility 由以下因素决定：

- mode。
- task type。
- policy。
- plugin status。
- server status。
- user approval。

## 7. Startup Strategy

MCP server 启动状态：

- not_configured。
- configured。
- starting。
- running。
- failed。
- disabled。
- blocked_by_policy。

Required server fail：

- 如果 task 依赖它，block。

Optional server fail：

- 降级并记录。

## 8. Tool Call Pairing

每个 tool call 必须有：

- call id。
- input。
- started event。
- completed/failed event。
- output summary。

孤儿 tool call 必须在 turn 结束时补齐 failure event。

## 9. Output Handling

输出分类：

- small output。
- large output。
- binary artifact。
- sensitive output。
- streaming output。

策略：

- small 直接记录。
- large 记录 head/tail/path。
- binary 记录 artifact path。
- sensitive redaction。
- streaming 写 runtime event，summary 入 ledger。

## 10. Tool Failure

Failure 类型：

- command_failed。
- timeout。
- permission_denied。
- policy_denied。
- server_unavailable。
- invalid_output。
- schema_mismatch。

每个 failure 必须有 recoverability：

- retryable。
- needs_user。
- needs_config。
- blocked。

## 11. MCP Tool Scope

MCP 工具不能默认全部暴露给 worker。

规则：

- worker 只看 WorkPacket allowlist。
- high-risk MCP 需要 approval。
- unknown MCP 默认 hidden。
- plugin-disabled MCP 不可见。

## 12. Resource Model

MCP resources 用于：

- repo context。
- indexed graph。
- external docs。
- project memory。

资源读取也需要 provenance。

## 13. Prompt Model

MCP prompts 是 workflow hint，不是强制规则。

使用时：

- 标注来源。
- 标注版本。
- 标注适用 mode。
- 与 project rules 冲突时 project wins。

## 14. P0 验收

- tool registry 可列出。
- MCP status 可解释。
- required/optional 行为不同。
- tool call/result 成对。
- policy denial 有 event。
- large output 不撑爆 context。
