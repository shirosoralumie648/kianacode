# Volume 13: Plugin 与 Skill 生态设计

## 1. 目标

Kiana 需要插件和 skill 生态，但不能让生态变成不可控风险。插件系统要服务 Project OS，而不是替代核心协议。

目标：

- 可安装。
- 可禁用。
- 可审计。
- 可限制权限。
- 可测试。
- 可归因。

## 2. 能力类型

| 类型 | 用途 |
| --- | --- |
| command | 新增 slash/CLI 命令 |
| skill | 工作流知识和方法 |
| agent | 专家角色或 worker 类型 |
| rule | 条件注入规则 |
| hook | 生命周期触发 |
| mcp | 外部工具连接 |
| template | 文档/packet/report 模板 |
| check | 验证器或审查器 |

## 3. Plugin Manifest

Manifest 字段：

- name。
- version。
- source。
- description。
- capabilities。
- permissions。
- commands。
- skills。
- agents。
- hooks。
- mcp_servers。
- compatibility。
- checks。

Manifest 不足以信任插件，还需要 receipt 和 policy。

## 4. Install Receipt

Receipt 记录：

- plugin id。
- source。
- version。
- hash。
- installed_at。
- installed_by。
- files installed。
- capabilities exposed。
- policy decision。

Receipt 用于：

- audit。
- uninstall。
- disable。
- reproducibility。

## 5. Skill 模型

Skill 包含：

- trigger。
- instructions。
- required inputs。
- outputs。
- failure modes。
- verification。
- examples。

Skill 不应：

- 悄悄扩大权限。
- 修改 policy。
- 直接宣称完成。
- 覆盖项目规则。

## 6. Command 模型

Command 字段：

- name。
- mode。
- input schema。
- output schema。
- required permissions。
- evidence behavior。
- failure behavior。

Command 必须能被 router 理解。

## 7. Agent 模型

Agent 字段：

- agent_type。
- allowed tasks。
- allowed tools。
- default budget。
- required review。
- output packet type。

Agent 不是人格设定，而是能力边界。

## 8. Rule 模型

Rule 类型：

- always。
- mode。
- path。
- glob。
- task type。
- domain。

Rule conflict 处理：

- explicit deny wins。
- project rule wins over plugin rule。
- user override wins if safe。
- conflict must be reported。

## 9. Hook 模型

Hook 生命周期：

- SessionStart。
- UserPromptSubmit。
- PreToolUse。
- PostToolUse。
- Stop。
- PreCompact。

Hook 权限：

- read-only。
- audit。
- blocking。
- mutating。

安全：

- blocking hook fail-closed。
- non-critical hook fail-soft。

## 10. Marketplace Trust

Marketplace 需要：

- source registry。
- signature/hash。
- verified publisher。
- compatibility。
- permission summary。
- install preview。

安装前展示：

- 将安装哪些能力。
- 需要哪些权限。
- 会写哪些文件。
- 是否启用 hooks/MCP。

## 11. Disabled Plugin

禁用后：

- command 不可见。
- skill 不注入。
- agent 不可派发。
- hook 不运行。
- MCP 不启动。

但保留：

- receipt。
- audit trail。
- historical events。

## 12. Skill Eval

Skill 上架前需要：

- static lint。
- trigger test。
- output format test。
- safety review。
- example run。

P0 可以只支持本地 skill eval。

## 13. 与 Reference 的关系

吸收：

- ruflo 的 plugin/workflow/swarm 生态思路。
- everything-claude-code 的 ecosystem taxonomy。
- superpowers 的 workflow skill。
- ECC 的 cross-harness packaging。
- ai-coding-guide 的中文教程式 onboarding。

不吸收：

- 只列 catalog。
- 无权限 manifest。
- 自动启用全部 hooks。
- 未签名 marketplace 默认信任。

## 14. P0 验收

- manifest 可校验。
- install receipt 可生成。
- disabled plugin 生效。
- command/skill/rule 可列出。
- hook/MCP 权限可解释。
- plugin 不可绕过 PolicyDecision。
