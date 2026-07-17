# Volume 21: Scheduler、依赖与优先级模型

## 1. 目标

Scheduler 是 Kiana Project OS 的任务选择器。它不负责写代码，也不负责做审查，而是回答：

- 哪些任务 Ready。
- 哪个任务最应该先做。
- 哪些任务可以并行。
- 哪些任务需要等待。
- 哪些任务必须升级到用户决策或 approval。

Scheduler 的输出必须可解释，不能只给一个排序结果。

## 2. 输入

Scheduler 输入：

- WorkflowState。
- Task Cards。
- Dependency Graph。
- current git state。
- path lock table。
- policy state。
- verification availability。
- user priority。
- blocker records。
- memory hints。

输入中，live state 优先于 memory。

## 3. 输出

Scheduler 输出 `ScheduleDecision`：

```json
{
  "schema_version": "kiana.schedule_decision.v1",
  "workflow_id": "wf_001",
  "selected": ["task_003"],
  "parallel_candidates": ["task_004", "task_005"],
  "skipped": [
    {
      "task_id": "task_006",
      "reason": "blocked_by_dependency",
      "dependency": "task_002"
    }
  ],
  "reasons": [
    "task_003 is ready",
    "task_003 has highest unblock value",
    "verification commands are available"
  ],
  "requires_approval": false
}
```

## 4. Dependency Graph

依赖图节点：

- Project Goal。
- Workstream。
- Milestone。
- Task Card。
- External Decision。
- Artifact。
- Verification。
- Approval。

边类型：

- blocks。
- requires。
- produces。
- verifies。
- supersedes。
- related。

## 5. Ready 判定

Task Ready 必须满足：

- status = ready。
- dependencies done。
- required artifacts exist。
- required decisions made。
- required approvals granted or not needed。
- allowed paths known。
- verification commands known or intentionally absent with reason。
- no active conflicting path lock。

任何一个条件不满足，都不能进入 dispatch。

## 6. Priority 信号

优先级由多个信号组合：

| 信号 | 意义 |
| --- | --- |
| user_priority | 用户明确优先级 |
| blocker_unblock_value | 能解除多少阻塞 |
| critical_path | 是否在关键路径上 |
| risk_reduction | 是否降低最大风险 |
| evidence_value | 是否能产出关键证据 |
| implementation_size | 是否是小而完整切片 |
| dependency_fanout | 被多少任务依赖 |
| verification_readiness | 是否可马上验证 |
| stale_risk | 是否拖久会过期 |

Scheduler 不应只按创建顺序选任务。

## 7. Critical Path

Critical Path 用于长期项目：

- release blocker。
- core protocol。
- data contract。
- verification。
- recovery。
- policy。

如果一个任务阻塞多个后续任务，它的优先级提高。

## 8. Parallel Candidate 判定

可以并行的条件：

- 都是 Ready。
- allowed files 不重叠。
- 不共享 high-risk files。
- 不都修改 schema。
- verification 可合并。
- policy 允许。
- worker budget 足够。

不能并行的情况：

- 同一 lock file。
- 同一 config。
- 同一 generated artifact。
- 互相依赖。
- 一个任务改接口，另一个依赖该接口。

## 9. Path Risk

高风险路径：

- lock files。
- release scripts。
- policy files。
- schema files。
- core runtime files。
- auth/secret files。
- plugin manifest。
- MCP config。

这些路径即使不重叠，也可能需要主 agent 串行处理。

## 10. Scheduler Modes

### 10.1 Conservative

用于：

- P0。
- release。
- policy。
- security。
- EDA。

行为：

- 少并行。
- 强 approval。
- 强 verification。

### 10.2 Balanced

用于：

- 日常 project。
- 中等任务。

行为：

- 允许非冲突并行。
- 保留主 agent 集成。

### 10.3 Aggressive

用于：

- 用户明确要求高并发。
- 风险低的文档/测试/参考审计。

行为：

- 更多 worker。
- 但仍有 path lock。

## 11. Starvation 防止

避免某些任务长期不做：

- blocked task 定期复查。
- low priority task 超时提升。
- stale task 标记。
- dependency chain 定期重算。

## 12. Blocked Task 复查

复查触发：

- dependency done。
- user decision arrived。
- artifact created。
- git state changed。
- memory refreshed。

复查后：

- blocked -> ready。
- blocked -> cancelled。
- blocked -> still blocked with updated reason。

## 13. 用户干预

用户可以：

- pin task。
- boost priority。
- defer task。
- cancel task。
- force serial。
- allow swarm。

但不能绕过：

- policy。
- approval。
- forbidden files。
- safety gate。

## 14. Scheduler 验收

P0 验收：

- `/project next` 输出可解释。
- blocked task 不被选中。
- dependency 完成后 task 可变 Ready。
- path 冲突任务不进入 swarm。
- 用户优先级能影响排序。
- high-risk task 自动升级到 approval。
