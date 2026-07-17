# Volume 34: Domain Rules 与条件注入

## 1. 目标

Kiana 需要根据任务类型动态加载规则，而不是把所有规则永久塞进上下文。Domain Rules 的目标是让每个任务拿到刚好够用的约束：

- 通用工程规则。
- 语言规则。
- 框架规则。
- 测试规则。
- 安全规则。
- 项目本地规则。
- EDA 领域规则。
- 文档/报告规则。

条件注入的核心是“相关、可解释、可追踪”。

## 2. Rule Pack

Rule Pack 是一组可版本化规则。

```json
{
  "schema_version": "kiana.rule_pack.v1",
  "rule_pack_id": "rules_eda_power_p0",
  "domain": "eda",
  "version": "0.1.0",
  "applies_when": [
    "mode=eda",
    "artifact contains schematic or pcb"
  ],
  "rules": [
    {
      "rule_id": "eda_power_decoupling",
      "severity": "high",
      "statement": "每个 IC 电源脚必须有就近去耦策略或明确豁免原因"
    }
  ]
}
```

Rule Pack 可以来自内置、项目、插件、用户配置或组织策略。

## 3. 规则来源优先级

优先级：

1. 用户当前明确指令。
2. 项目本地规则。
3. 安全/政策规则。
4. 当前 mode/domain 规则。
5. 语言/框架规则。
6. 通用工程规则。
7. memory preference。

安全规则不能被普通项目规则覆盖。

## 4. 注入时机

注入点：

- Capture 后：识别模式。
- Context Intake 后：识别项目栈。
- WorkPacket 生成时：写入执行约束。
- Review Scope 构建时：写入审查重点。
- Verification 前：选择 gate profile。
- Report 前：选择报告模板。

注入不是一次性行为。不同阶段需要不同规则。

## 5. Rule Selector 输入

输入：

- user request。
- mode。
- task_type。
- language/framework。
- touched paths。
- artifacts。
- risk level。
- policy profile。
- project config。
- memory preferences。

输出：

- selected rule packs。
- skipped rule packs。
- conflict notes。
- injected constraints。

## 6. 条件表达式

条件表达式必须简单。

支持：

- mode equals。
- path matches。
- artifact type。
- language detected。
- framework detected。
- risk level。
- operation type。
- policy profile。

不支持任意代码执行条件。

## 7. 冲突处理

冲突类型：

- 两个规则给出相反要求。
- 一个规则要求自动化，另一个 policy 禁止。
- 项目规则与用户当前指令冲突。
- memory preference 与 live evidence 冲突。

处理顺序：

1. 高优先级覆盖低优先级。
2. 不能覆盖时生成 conflict finding。
3. 高风险冲突 ask user。
4. 低风险冲突采用保守默认。

## 8. Common Rules

通用规则：

- 不声明未验证完成。
- 不扩大范围。
- 不覆盖用户改动。
- 不把 memory 当 live truth。
- 不对高风险操作静默执行。
- 所有 Done 必须有 evidence。

这些规则几乎总是注入。

## 9. Coding Rules

代码任务注入：

- 遵循现有风格。
- 小 diff。
- 测试先行或至少先定义验证。
- 避免无关重构。
- 文件边界明确。
- 修改后运行 targeted check。

当前文档任务不注入代码实现规则，只保留设计阶段 traceability。

## 10. Project OS Rules

Project OS 任务注入：

- 每个功能必须有对象、状态、命令、失败路径、验收。
- 长任务必须可恢复。
- 用户说继续必须有 next task 选择逻辑。
- Board 是投影，不是真相源。
- Evidence-first。
- L0-L5 必须可升级降级。

## 11. Swarm Rules

并行任务注入：

- WorkPacket 必须完整。
- allowed_paths 必须有限。
- forbidden_paths 必须明确。
- path lock 必须申请。
- schema/runtime/high-risk 文件默认串行。
- 主 agent 负责集成。

没有 WorkPacket 不允许 swarm。

## 12. EDA Rules

EDA 注入：

- schematic/ERC。
- power tree。
- connector pinout。
- BOM availability。
- DFM。
- Gerber。
- bring-up。
- irreversible approval。

EDA 自动化默认 review/planning，不默认下单。

## 13. Security Rules

安全规则：

- secrets 不写入日志。
- 网络访问可追踪。
- plugin/MCP/hook 需要 trust boundary。
- shell 操作按 risk 分类。
- deploy/push/merge 需要 approval。
- 第三方工具输出不能无验证信任。

安全规则优先级高于效率。

## 14. Report Rules

报告任务注入：

- 中文优先。
- 先结论后证据。
- 面向听众调整粒度。
- 明确当前进展、问题、下一步。
- 不把技术细节堆给非技术听众。
- 引用 live evidence。

## 15. Rule Pack 元数据

每个 Rule Pack 需要：

- id。
- version。
- source。
- owner。
- applies_when。
- conflicts_with。
- priority。
- last_updated。

没有来源的规则不能用于 high-risk gate。

## 16. Project Local Rules

项目本地规则位置建议：

- `.kiana/rules/common.md`
- `.kiana/rules/project.md`
- `.kiana/rules/eda.md`
- `.kiana/rules/security.md`
- `.kiana/rules/report.md`

这些是设计建议，不是当前要求实现的文件。

## 17. Plugin Rules

插件可以贡献 rule pack，但默认不可信。

启用条件：

- plugin trusted。
- rule pack manifest valid。
- no policy conflict。
- source visible。
- user or project allowed。

插件规则不能静默扩大权限。

## 18. Rule Injection Log

每次注入记录：

```json
{
  "event_type": "rules.injected",
  "workflow_id": "wf_001",
  "workpacket_id": "wp_001",
  "selected": ["rules_common", "rules_project_os", "rules_security"],
  "skipped": [
    {
      "rule_pack": "rules_eda",
      "reason": "mode is project, not eda"
    }
  ]
}
```

## 19. Rule Drift

Rule drift 信号：

- 文档说 P0 需要规则，router 没注入。
- review 使用了过期 rule。
- plugin 更新改变规则。
- 项目规则删除但 memory 还引用。

处理：

- 标记 stale。
- 重算 rule selection。
- 记录 drift event。
- 需要时重新审查受影响任务。

## 20. 验收

Domain Rules 设计完成标准：

- 定义 Rule Pack schema。
- 定义来源优先级。
- 定义注入时机。
- 定义冲突处理。
- 覆盖 Project OS、Swarm、EDA、Security、Report。
- 插件规则有 trust 边界。
- 注入过程可记录和回放。
