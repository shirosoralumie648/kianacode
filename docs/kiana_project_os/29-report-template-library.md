# Volume 29: 中文报告模板库

## 1. 目标

报告模板让 Kiana 输出稳定、可读、可汇报的中文文本。模板不能替代事实来源，所有结论都必须来自 evidence。

## 2. 通用报告原则

- 先结论。
- 再证据。
- 区分完成/未完成/阻塞/未知。
- 不夸大。
- 不把计划当进展。
- 不把 memory 当当前事实。

## 3. Progress Report 模板

```text
当前进展：
- 目标：<goal>
- 已完成：<done with evidence>
- 正在推进：<doing>
- 当前阻塞：<blocked>
- 风险：<risks>
- 下一步：<next>
```

## 4. Teacher Report 模板

```text
老师，这个项目目前的定位是 <thesis>。
现在已经完成的是 <verified progress>。
目前主要问题是 <blockers>。
下一步我准备先做 <next milestone>，因为 <reason>。
```

## 5. Handoff 模板

```text
Handoff:
- Workflow: <id>
- Goal: <goal>
- Current node: <node>
- Last verified: <evidence>
- Dirty state: <dirty>
- Next task: <task>
- Do not touch: <forbidden>
- Blockers: <blockers>
```

## 6. Audit Report 模板

```text
审计范围：<scope>
结论：<pass/fail/blocked>
阻断问题：
1. <finding> - evidence: <evidence>
高风险问题：
1. <finding>
建议 P0 修复：
1. <task>
```

## 7. Commercial Readiness 模板

```text
商用化状态：<status>
本地阻塞：<local_blockers>
外部阻塞：<external_blockers>
已验证证据：<proofs>
缺口：<gaps>
下一步：<next>
```

## 8. Swarm Report 模板

```text
并行执行结果：
- 派发 worker：<count>
- 完成：<completed>
- 拒绝集成：<rejected>
- 冲突：<conflicts>
- 统一验证：<verification>
- 下一步：<next>
```

## 9. EDA Review 模板

```text
EDA 审查结果：
- 资料完整性：<artifacts>
- 原理图风险：<schematic>
- BOM 风险：<bom>
- DFM 风险：<dfm>
- Bring-up 建议：<bringup>
- 需要人工确认：<approvals>
```

## 10. Block Report 模板

```text
当前阻塞：
- 节点：<node>
- 原因：<reason>
- 证据：<evidence>
- 需要你决定：<decision>
- 可选方案：<options>
- 推荐默认：<default>
```

## 11. Release Report 模板

```text
发布检查：
- build：<status>
- package：<status>
- install：<status>
- smoke：<status>
- security：<status>
- license：<status>
- blockers：<blockers>
```

## 12. Memory Report 模板

```text
Memory 状态：
- 可用高置信记忆：<count>
- stale：<count>
- 本轮使用：<used>
- 被拒绝：<rejected>
- 需要刷新：<refresh>
```

## 13. 报告 source label

标签：

- `[live]`
- `[verified]`
- `[memory-derived]`
- `[historical]`
- `[unknown]`
- `[blocked]`

## 14. 禁止话术

禁止：

- “应该完成了”。
- “看起来没问题”。
- “基本可商用”但无 release proof。
- “已修复”但无验证。
- “无风险”但未审计。

## 15. 验收

P0 验收：

- `/report progress` 使用模板。
- audit report 有 severity。
- blocker report 有 options。
- EDA report 有 approval needs。
- source labels 可见。
