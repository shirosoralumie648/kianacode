# Volume 08: Trust、Policy、Plugin、MCP 与 Hooks

## 1. Trust 模型目标

Kiana 要能用于长期自用和未来企业交付，所以不能把所有能力默认可信。Trust 模型要覆盖：

- shell。
- file edit。
- network。
- plugin。
- MCP。
- hooks。
- worker。
- memory。
- EDA。

## 2. PolicyDecision

每次高价值动作都应产生 PolicyDecision。

字段：

- subject。
- capability。
- target。
- requested_action。
- decision。
- reason。
- requires_approval。
- policy_source。
- audit_event_id。

decision：

- allow。
- deny。
- ask。
- allow_with_constraints。

## 3. Capability 分类

| Capability | 风险 |
| --- | --- |
| read_file | low/medium |
| write_file | medium/high |
| shell_command | medium/high |
| network_access | high |
| install_dependency | high |
| plugin_enable | high |
| mcp_tool_expose | high |
| hook_register | high |
| worker_dispatch | medium/high |
| memory_write | medium |
| push_merge_deploy | critical |
| hardware_order | critical |

## 4. Shell Policy

允许：

- workspace 内只读命令。
- 测试命令。
- 构建命令。
- lint/typecheck。

ask：

- 删除。
- reset/checkout。
- 修改权限。
- 安装依赖。
- 网络下载。
- 启动长期服务。

deny：

- 未授权读取 secrets。
- 上传代码到未知地址。
- 删除用户未确认文件。

## 5. File Edit Policy

允许：

- allowed_paths 内 exact edit。
- 新增 task 相关测试。
- 更新本 workflow artifact。

ask：

- 跨 scope。
- lock file。
- config。
- release scripts。
- policy files。

deny：

- `.env`。
- secrets。
- 用户明确 forbidden。
- worker 修改非 allowed files。

## 6. Network Policy

默认：

- ask。

allowlist：

- 官方 docs。
- package registry if approved。
- configured APIs。

记录：

- host。
- purpose。
- data sensitivity。
- response summary。

## 7. Plugin Trust

Plugin 必须有：

- manifest。
- version。
- source。
- hash。
- capabilities。
- permissions。
- compatibility。
- install receipt。

启用前检查：

- manifest schema。
- hash/signature。
- policy allow。
- disabled list。
- capability exposure。

## 8. MCP Trust

MCP server 字段：

- name。
- command/url。
- source。
- required/optional。
- visibility。
- tools exposed。
- startup status。
- trust level。

规则：

- MCP 工具不默认全暴露。
- required server 失败可 block。
- optional server 失败只降级。
- tool visibility 按 mode/task 限制。

## 9. Hooks Policy

Hook 生命周期：

- SessionStart。
- UserPromptSubmit。
- PreToolUse。
- PostToolUse。
- Stop。
- PreCompact。

安全 hook：

- fail-closed。

普通 hook：

- fail-soft。

记录：

- lifecycle。
- hook id。
- exit status。
- payload summary。
- decision。

## 10. Worker Policy

Worker 权限来自 WorkPacket。

允许：

- allowed files。
- listed commands。
- listed tools。

禁止：

- install plugin。
- change policy。
- push/merge。
- access forbidden files。
- expand scope silently。

## 11. Memory Policy

Memory write 需要：

- source。
- confidence。
- evidence。
- retention。

高风险 memory：

- 用户偏好。
- 安全决策。
- 商业化状态。
- release readiness。

这些必须可追溯。

## 12. EDA Policy

需要 approval：

- BOM 下单。
- PCB 打样。
- 高压/电池建议执行。
- 安规/认证结论。
- 自动替换关键器件。

P0 输出只能是 review 和 plan。

## 13. Approval Gate

Approval request 内容：

- action。
- why needed。
- risk。
- rollback。
- alternatives。
- exact command or operation。

Approval 结果：

- approved。
- rejected。
- approved_once。
- approved_for_workflow。
- needs_more_info。

禁止：

- 把旧 approval 用到新目标。
- 模糊 approval。
- approval 后改变命令不重审。

## 14. Audit Trail

所有 policy decision 写入 eventlog。

Report 中应能回答：

- 哪些高风险操作被请求。
- 哪些被允许。
- 哪些被拒绝。
- 谁批准。
- 基于什么证据。
