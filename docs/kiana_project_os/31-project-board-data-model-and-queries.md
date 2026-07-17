# Volume 31: Project Board 数据模型与查询语义

## 1. 目标

Project Board 是 Kiana 的项目管理表面。它不是一个简单 todo list，而是 WorkflowRun、Task Card、WorkPacket、Evidence、Blocker、Decision、Gate 的统一投影。

它要回答：

- 当前项目到底有哪些目标。
- 每个目标拆成了哪些工作流、里程碑和任务。
- 哪些任务 Ready，哪些 Blocked，哪些 Done。
- 哪些任务适合并行，哪些必须串行。
- 当前最大的交付风险在哪里。
- 用户说“继续”时应该推进哪一项。

Project Board 本身不执行任务。它只维护可解释的项目状态和可查询视图。

## 2. Board 与 Workflow 的关系

一个 Project Board 可以包含多个 WorkflowRun：

- 一个长期产品项目。
- 一个独立 bugfix。
- 一个 release 审计。
- 一个 EDA 项目审查。
- 一个参考仓库迁移计划。

Board 是长期容器，WorkflowRun 是一次执行闭环。

关系：

```text
ProjectBoard
  -> Goals
  -> Workstreams
  -> Milestones
  -> WorkflowRuns
  -> TaskCards
  -> WorkPackets
  -> EvidenceEvents
  -> Decisions
  -> Blockers
```

Board 必须允许用户跨会话恢复，而不是只能看到当前对话里的任务。

## 3. Board 数据对象

最小对象：

```json
{
  "schema_version": "kiana.project_board.v1",
  "project_id": "proj_kiana",
  "name": "Kiana Personal Project OS",
  "created_at": "2026-07-09T10:00:00+08:00",
  "updated_at": "2026-07-09T18:00:00+08:00",
  "default_mode": "project",
  "status": "active",
  "goals": ["goal_project_os"],
  "active_workflows": ["wf_20260709_design"],
  "views": ["kanban", "wbs", "risk", "release", "eda"],
  "policy_profile": "personal_local",
  "memory_scope": "project"
}
```

字段含义：

| 字段 | 含义 |
| --- | --- |
| project_id | 稳定项目 ID |
| name | 人类可读名称 |
| default_mode | 默认入口模式 |
| status | active/paused/archived/blocked |
| goals | 当前目标集合 |
| active_workflows | 未完成 workflow |
| views | 可用投影视图 |
| policy_profile | 权限默认配置 |
| memory_scope | memory 查询边界 |

## 4. Goal 对象

Goal 是用户意图的长期版本。

```json
{
  "goal_id": "goal_project_os",
  "title": "把 Kiana 做成个人项目操作系统",
  "thesis": "目标管理、记忆、代码理解、并行执行、验证交付在一个闭环里",
  "success_criteria": [
    "支持 /project 长任务恢复",
    "支持 WBS/Kanban 任务拆解",
    "支持 evidence-first done",
    "支持 bounded swarm 并行执行",
    "支持 EDA 审查入口"
  ],
  "not_building": [
    "P0 不做云端团队协作",
    "P0 不做自动 PCB 下单",
    "P0 不做无限制 worker swarm"
  ],
  "status": "active"
}
```

Goal 不应频繁改变。需求变化应形成 ChangeRequest，而不是静默覆盖 Goal。

## 5. Workstream 对象

Workstream 是 PMP/WBS 结构中的中层分组。

示例：

- Runtime。
- Project OS。
- Memory。
- Repo Intelligence。
- Bounded Swarm。
- Policy。
- EDA。
- Commercial Readiness。

Workstream 字段：

```json
{
  "workstream_id": "ws_policy",
  "goal_id": "goal_project_os",
  "title": "Trust / Policy / Approval",
  "owner": "main_agent",
  "status": "active",
  "risk_level": "high",
  "milestones": ["ms_policy_p0"],
  "depends_on": ["ws_runtime"],
  "outputs": ["policy_profile", "approval_log", "tool_permission_matrix"]
}
```

## 6. Milestone 对象

Milestone 必须可验收。

坏 milestone：

- “完善 policy”。
- “做好项目管理”。
- “支持并行”。

好 milestone：

- “实现 P0 Project Board schema、状态投影和 /project status 输出契约”。
- “实现 approval required/approved/rejected 的事件流和恢复行为”。
- “实现 allowed files/path lock/workpacket 合并策略”。

Milestone 字段：

```json
{
  "milestone_id": "ms_project_board_p0",
  "title": "Project Board P0",
  "definition_of_done": [
    "board schema defined",
    "kanban view can be generated",
    "blocked reason visible",
    "next task selection explainable"
  ],
  "verification": [
    "schema fixture validates",
    "sample project renders board summary",
    "stale workflow is marked correctly"
  ],
  "status": "ready"
}
```

## 7. Kanban 状态

Kiana Kanban 不是自由文本列。列必须能映射到执行状态。

标准列：

| Column | 语义 |
| --- | --- |
| Backlog | 已捕获但未拆解 |
| Spec | 正在澄清规格 |
| Ready | 可执行 |
| In Progress | 正在执行 |
| Review | 等待审查或 gate |
| Blocked | 被依赖/决策/权限/失败阻塞 |
| Done | 已有 evidence 的完成 |
| Archived | 已关闭且不再参与调度 |

禁止状态：

- done_without_evidence。
- blocked_without_reason。
- in_progress_without_owner。
- ready_without_verification。

## 8. WBS 视图

WBS 视图强调结构：

```text
Goal
  Workstream
    Milestone
      Task Card
        WorkPacket
```

WBS 用于：

- 长期项目说明。
- PMP 式拆解。
- 与老师/团队汇报。
- 发现范围膨胀。
- 判断哪些任务不是当前 milestone 必需。

WBS 不应用来直接调度 worker。调度要用 ready queue 和 dependency graph。

## 9. Risk 视图

Risk 视图按风险聚合：

- release blocker。
- security blocker。
- data contract drift。
- missing verification。
- stale memory。
- dirty git state。
- conflicting worker edits。
- EDA irreversible action。

Risk 视图必须显示：

- 风险等级。
- 影响范围。
- 证据来源。
- owner。
- next mitigation。
- deadline if any。

## 10. Release 视图

Release 视图用于商用化：

- 当前 release 目标。
- 必须通过的 gates。
- 本地阻塞。
- 外部阻塞。
- 缺失证据。
- 未决 approval。
- 可交付 artifact。
- 不可交付原因。

Release 视图不能只显示百分比。百分比必须能下钻到 blocker ledger。

## 11. EDA 视图

EDA 视图用于硬件项目：

- schematic review。
- PCB layout review。
- BOM availability。
- DFM constraints。
- Gerber readiness。
- power tree。
- interface pinout。
- bring-up plan。
- irreversible action approval。

EDA 视图复用同一套 Evidence/Gate/Approval，不另起系统。

## 12. 查询语义

Project Board 必须支持 query，而不是只能输出一整页摘要。

核心查询：

| Query | 用途 |
| --- | --- |
| next_task | 用户说继续时选择下一项 |
| blocked_tasks | 查阻塞 |
| stale_tasks | 查长期未更新任务 |
| ready_parallel | 查可并行候选 |
| release_blockers | 查商用阻塞 |
| missing_evidence | 查没有证据的完成声明 |
| decision_needed | 查需要用户拍板的灰区 |
| eda_risks | 查硬件风险 |

## 13. next_task 查询

`next_task` 返回一个解释型结果：

```json
{
  "query": "next_task",
  "selected_task": "task_policy_profile",
  "why": [
    "ready",
    "unblocks 3 downstream tasks",
    "verification command available",
    "touches no locked path"
  ],
  "alternatives": [
    {
      "task_id": "task_dashboard_projection",
      "why_not": "depends on board schema"
    }
  ],
  "mode": "balanced"
}
```

Router 使用这个结果决定是否进入 `/task`、`/project` 或 `/swarm`。

## 14. blocked_tasks 查询

Blocked 查询必须区分原因：

- dependency_blocked。
- decision_blocked。
- approval_blocked。
- evidence_blocked。
- verification_blocked。
- external_blocked。
- policy_blocked。
- conflict_blocked。
- stale_context_blocked。

每条 blocked task 必须有解除条件。

## 15. ready_parallel 查询

并行候选输出：

```json
{
  "query": "ready_parallel",
  "candidates": [
    {
      "task_id": "task_docs_audit",
      "allowed_paths": ["docs/reference_audit/**"],
      "conflict_score": 0.1
    },
    {
      "task_id": "task_report_templates",
      "allowed_paths": ["docs/kiana_project_os/29-*"],
      "conflict_score": 0.1
    }
  ],
  "not_parallel": [
    {
      "task_id": "task_schema_runtime",
      "reason": "schema file high risk"
    }
  ]
}
```

并行候选只是建议，实际 dispatch 还要经过 policy 和 path lock。

## 16. Board 更新规则

Board 更新来源：

- 用户输入。
- WorkflowRun event。
- Task status event。
- Evidence event。
- Review result。
- Approval event。
- Git state probe。
- Memory refresh。

禁止直接手改投影结果。投影应从事件和对象生成。

## 17. 投影一致性

Project Board 是投影，不是唯一真相。

真相来源优先级：

1. Append-only EventLog。
2. Workflow state file。
3. Task Card objects。
4. Evidence Ledger。
5. Review/Approval records。
6. Derived board projection。

如果 Board 与 EventLog 冲突，应重建 Board。

## 18. 搜索与过滤

搜索字段：

- title。
- goal。
- owner。
- status。
- risk_level。
- path。
- tag。
- evidence type。
- blocker reason。
- decision owner。

过滤组合：

```text
status=blocked AND risk>=high AND workstream=release
status=ready AND verification=available AND path_conflict=false
mode=eda AND approval_required=true
```

## 19. 排序规则

默认排序：

1. blocked release-critical。
2. ready critical path。
3. user priority。
4. high unblock value。
5. stale risk。
6. small verified slice。
7. creation time。

排序必须解释，不允许黑盒。

## 20. Board 输出格式

人类摘要：

```text
当前项目：Kiana Personal Project OS
状态：active
Ready：4
In Progress：1
Blocked：3
最大风险：policy approval model 未定
下一步：完成 WorkPacket schema deep dive
```

机器输出：

```json
{
  "schema_version": "kiana.board_summary.v1",
  "project_id": "proj_kiana",
  "counts": {
    "ready": 4,
    "in_progress": 1,
    "blocked": 3,
    "done": 18
  },
  "top_risks": ["policy approval model 未定"],
  "next_task": "task_workpacket_schema"
}
```

## 21. 冲突处理

冲突类型：

- 同一 task 多状态。
- evidence 指向不存在 task。
- blocker 已解除但状态仍 blocked。
- task done 但 verification failed。
- workstream archived 但 task active。

处理：

- 优先 EventLog。
- 生成 consistency finding。
- 标记 board projection stale。
- 触发 repair projection。
- 高风险冲突升级给用户。

## 22. 验收

Project Board 设计可验收标准：

- 能表示 Goal/Workstream/Milestone/Task/WorkPacket。
- 能输出 Kanban/WBS/Risk/Release/EDA 五类视图。
- 能回答 next_task、blocked_tasks、ready_parallel。
- 能解释排序和跳过原因。
- 能从 EventLog 重建。
- 能发现 done_without_evidence。
- 能将 `/eda` 映射进同一套项目对象。
