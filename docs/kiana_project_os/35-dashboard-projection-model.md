# Volume 35: Dashboard 投影模型

## 1. 目标

Dashboard 是 Kiana 给用户看的项目驾驶舱。它不替代 CLI/TUI，也不替代文档，而是把 Project Board、Evidence Ledger、Gate Result、Blocker Ledger、Decision Log 投影成可扫描状态。

Dashboard 要回答：

- 现在项目在哪里。
- 下一步做什么。
- 哪些东西卡住。
- 哪些完成有证据。
- 哪些风险需要人类决策。
- 商用化/发布还差什么。

## 2. Dashboard 不是事实源

Dashboard 是 projection。

事实源：

- EventLog。
- WorkflowState。
- TaskCard。
- EvidenceLedger。
- ApprovalLog。
- BlockerLedger。
- Git/command live probes。

Dashboard 可以缓存，但必须可重建。

## 3. Projection 对象

```json
{
  "schema_version": "kiana.dashboard_projection.v1",
  "project_id": "proj_kiana",
  "generated_at": "2026-07-09T20:00:00+08:00",
  "freshness": "fresh",
  "summary": {
    "status": "active",
    "mode": "project",
    "health": "yellow",
    "next_action": "complete WorkPacket schema deep dive"
  },
  "cards": [],
  "alerts": [],
  "metrics": {}
}
```

freshness 可为：

- fresh。
- stale。
- partial。
- inconsistent。

## 4. 首页信息架构

首页分区：

- Project Header。
- Next Action。
- Health Summary。
- Workstream Progress。
- Blockers。
- Decisions Needed。
- Evidence Coverage。
- Recent Activity。
- Release Readiness。

不要把所有 event 展开到首页。

## 5. Project Header

显示：

- 项目名。
- 当前目标。
- 当前 mode。
- 当前 workflow。
- 最近更新时间。
- 当前 branch/worktree。
- policy profile。

如果 git state dirty，要显示但不恐吓用户。

## 6. Next Action Card

Next Action 来源于 Scheduler。

字段：

- selected task。
- why now。
- required approval。
- estimated mode。
- can_parallelize。
- verification available。

示例：

```json
{
  "card_type": "next_action",
  "task_id": "task_dashboard_projection",
  "title": "Define Dashboard projection model",
  "why": [
    "needed by /project status",
    "unblocks reporting surface",
    "design-only low conflict"
  ],
  "entry_command": "/task task_dashboard_projection"
}
```

## 7. Health Summary

Health 不是完成百分比。

状态：

- green：目标清楚，next action ready，无阻塞 gate。
- yellow：可推进，但有风险或缺证据。
- red：关键阻塞或安全/发布 gate failed。
- gray：上下文不足或 projection stale。

Health 必须给原因。

## 8. Progress Metrics

可显示指标：

- task counts by status。
- milestone completion。
- evidence coverage。
- verification pass rate。
- blocker count。
- decision count。
- stale task count。
- release gate pass count。

禁止单独使用“90% 完成”而没有分解。

## 9. Evidence Coverage

Evidence Coverage 衡量完成声明可信度。

```json
{
  "done_tasks": 18,
  "done_with_evidence": 18,
  "done_without_evidence": 0,
  "failed_verification": 1,
  "missing_review": 2
}
```

如果 done_without_evidence > 0，Health 至少 yellow。

## 10. Blocker Panel

Blocker 显示：

- blocker id。
- type。
- severity。
- owner。
- affected tasks。
- unblock condition。
- age。

Blocker age 用于防止长期卡住没人处理。

## 11. Decision Panel

Decision Panel 显示：

- 需要用户决定。
- 已默认的决策。
- 已拒绝的 approval。
- 即将过期的 approval。
- 被 supersede 的旧决策。

高风险 approval 要突出显示。

## 12. Workstream Progress

Workstream 不是百分比条即可。

每个 workstream 显示：

- status。
- current milestone。
- ready tasks。
- blocked tasks。
- top risk。
- latest evidence。

这样用户能判断为什么一个方向慢。

## 13. Release Readiness

Release Readiness 维度：

- local blockers。
- external blockers。
- security blockers。
- test blockers。
- docs blockers。
- packaging blockers。
- policy blockers。
- user approval blockers。

每个 blocker 必须能跳到 evidence。

## 14. EDA Dashboard

EDA 模式显示：

- schematic status。
- PCB status。
- BOM status。
- DFM status。
- Gerber export status。
- bring-up checklist。
- irreversible approval。

硬件风险不应混在普通代码 blocker 里。

## 15. Activity Feed

Activity Feed 显示最近事件：

- workflow created。
- task moved。
- evidence added。
- gate passed/failed。
- decision made。
- approval requested。
- blocker opened/closed。

Feed 应支持折叠，避免淹没用户。

## 16. Projection Refresh

刷新触发：

- workflow event appended。
- task status changed。
- evidence added。
- gate result changed。
- git state changed。
- user asks status/report。
- resume。

刷新失败时显示 stale，而不是继续展示旧状态。

## 17. 数据一致性

Dashboard 必须检测：

- task count mismatch。
- missing evidence target。
- blocker references missing task。
- decision references old workflow。
- stale projection timestamp。

发现不一致：

- 标记 inconsistent。
- 输出 repair suggestion。
- 高风险时阻止 ship。

## 18. 用户操作

Dashboard 可提供操作入口：

- continue。
- open next task。
- view blockers。
- approve/reject。
- generate report。
- refresh context。
- run audit。

操作仍走 Router/Policy，不绕过 gate。

## 19. CLI/TUI 输出关系

CLI 输出应和 Dashboard 一致。

`/project status` 是 Dashboard 的文本投影。

`/report` 是 Dashboard + Evidence 的叙述投影。

`/audit` 是 Dashboard + Gate 的风险投影。

## 20. 验收

Dashboard 设计完成标准：

- 明确 projection 非事实源。
- 定义首页分区。
- 定义 health/freshness。
- 定义 next action、blocker、decision、evidence、release readiness。
- 支持 EDA 投影。
- 支持 stale/inconsistent 检测。
- 所有操作回到 Router/Policy。
