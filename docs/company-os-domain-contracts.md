# Kiana CompanyOS 领域合同与业务生命周期

> 文档性质：规范目标与领域合同（Normative Target）。
>
> 本文补齐 CompanyOS 的“Company”一侧：目标、项目、交付、验收、运营和结果。它不替代 `company-os-design.md` 的总体原则、`company-os-security-constitution.md` 的安全宪法或 `company-os-implementation-outline.md` 的工程切片。
>
> 当前 checkout 的能力上限仍以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文中的对象和状态机未被实现或测试证明时，不得写成当前能力。

> **本文速览（导读，非规范）**
>
> - **讲什么**："公司业务"一侧的对象合同——从目标（Objective）、立项（Initiative）、项目（Project）、里程碑（Milestone），到验收（Acceptance）、交付（Delivery）、结果测量（Outcome），以及变更（ChangeRequest）、风险（Risk）和事故（Incident）的字段、状态机、命令/事件矩阵和规则；另含验收标准四层派生与冻结规则，以及运行恢复合同（RunSnapshot、调用账本）、任务图依赖不变量与 claim、委派信封（DelegationPacket）和审批决定事件。
> - **回答的问题**："一件工作为什么值得做、做到什么算完、谁说了算、交付之后目标到底实现没实现。"
> - **核心思想**：执行事实（Agent 跑成功了）和公司事实（业务目标实现了）是两条链，前者不能自动推出后者；每台状态机的每条转移都必须收敛到明确终态或归档出口，不允许死端。
> - **什么时候读**：实现或修改业务领域对象时；想理解"为什么 Receipt 不等于业务成功"时。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典。

## 1. 目的和边界

Kiana 当前已经有一套较完整的 Agent 控制面词汇：

```text
Role → AgentTemplate → Cell → WorkPacket → CapabilityExecution → Evidence → Receipt
```

这套词汇能回答“谁在什么权限下执行了什么”，但还不能完整回答：

- 这项工作为什么值得做；
- 它服务于哪个目标和项目；
- 项目是否仍然值得投资；
- 交付标准何时冻结、谁能修改；
- 谁真正接受了结果；
- 变更、风险和事故如何影响交付；
- 交付后是否产生了预期业务结果。

因此 CompanyOS 采用两条相互关联、但不能混淆的事实链：

```text
Company facts
Objective → Initiative → Project → Milestone → Acceptance → Outcome

Execution facts
WorkPacket → Cell → Run → Invocation → Evidence → Review → Receipt
```

二者通过 `project_id`、`milestone_id`、`work_packet_id`、`acceptance_id` 和 `outcome_id` 关联，但不能把执行成功自动推导为业务成功。

### 1.1 非目标

本文不把以下内容纳入个人本地 CompanyOS 核心：

- 企业租户、跨组织 RBAC 和远程 worker；
- 支付、签约、出票、下单等真实外部副作用；
- 客户 CRM、税务和完整财务总账；
- 无限 Agent swarm、自由消息总线或共享 transcript；
- 让自然语言报告替代服务端状态和 Receipt。

这些能力未来可以作为独立 bounded context 或 adapter，但必须复用同一个 ControlPlane、Broker、身份、审批、幂等和对账边界。

## 2. 领域分层

### 2.1 公司事实层

公司事实层描述价值和交付，不描述某个模型调用：

```text
Organization
  ├── Portfolio
  │     └── Program
  │            └── Project
  ├── Objective
  └── Initiative
```

- `Organization`：个人 Company 或未来租户根；
- `Portfolio`：一组目标、投入和优先级；
- `Program`：共享目标、依赖或风险的一组项目；
- `Project`：具有范围、基线、工作区、预算和关闭条件的交付聚合；
- `Objective`：带时间窗、指标和目标值的结果承诺；
- `Initiative`：进入项目之前的想法、机会或问题。

### 2.2 交付治理层

```text
Project
  ├── Charter
  ├── Plan
  ├── Milestone
  ├── WorkPacket
  ├── Risk
  ├── ChangeRequest
  ├── Acceptance
  ├── Delivery
  └── Outcome
```

该层负责把目标转为可交付工作，并保留范围、验收和结果的版本关系。

### 2.3 Agent 执行层

```text
Department
  → RoleSpec
  → AgentTemplate
  → AgentInstance / Cell
  → WorkPacket
  → CapabilityGrant / BudgetLease
```

`WorkPacket` 是跨部门正式交接单位；`Cell` 是有界的执行责任单元；`BudgetLease` 是运行时资源租约。它们不能成为项目财务预算、客户合同或业务收入的替代品。

### 2.4 运行与证据层

```text
Session → Turn → Run → Invocation → RuntimeEvent
                                      ├── Artifact
                                      ├── Review
                                      └── Receipt
```

- `Session` 是长期会话容器；
- `Turn` 是一次交互边界；
- `Run` 是一次执行生命周期；
- `Invocation` 是一次可恢复的能力请求；
- `RuntimeEvent` 是运行事实；
- `Artifact` 是持久工件；
- `Receipt` 是事件和证据的事实投影。

Transcript 只能是 UI 视图，不得作为 Company 或 Runtime 的唯一事实源。

## 3. Canonical 聚合目录

以下目录是目标合同（顶层聚合与 Project 版本化工件）。对象字段可随 schema 版本演进，但不能在不同 crate 或文档中重新定义同名语义。

| 聚合 | 解决的问题 | 最小关键字段 | canonical owner |
|---|---|---|---|
| `Organization` | 谁拥有目标、策略和资源 | `organization_id`, owner, policy profile, status | `kiana-domain` |
| `Objective` | 为什么做、什么算结果 | metric, baseline, target, period, owner | `kiana-domain` |
| `Portfolio` | 如何在多个目标间投资 | objectives, projects, priority, budget baseline | `kiana-domain` |
| `Program` | 哪些项目共享依赖或风险 | projects, dependencies, risk owner | `kiana-domain` |
| `Initiative` | 想法如何进入立项 | problem, hypothesis, sponsor, score, decision | `kiana-domain` |
| `Project` | 一个交付单元的生命周期 | charter, scope, baseline, milestones, sponsor, status | `kiana-domain` |
| `Charter` | 项目为什么被批准、边界是什么 | objective_refs, scope, success_criteria, non_goals, budget_ref, go_no_go, version | `kiana-domain` |
| `Plan` | 项目如何分批交付和派工 | charter_ref, milestone_refs, packet_refs, dependencies, version | `kiana-domain` |
| `Milestone` | 如何观察阶段性进展 | deliverables, due date, acceptance criteria, status | `kiana-domain` |
| `WorkPacket` | 如何正式派工和转移责任 | goal, inputs, paths, acceptance, owner, acceptor | `kiana-domain` |
| `ChangeRequest` | 如何改变冻结的范围或验收 | requested delta, impact, decision, new baseline | `kiana-domain` |
| `Risk` | 如何在事故前记录不确定性 | probability, impact, mitigation, owner, trigger | `kiana-domain` |
| `Incident` | 如何处理已发生的异常或未知结果 | severity, timeline, impact, response, escalation | `kiana-domain` |
| `Acceptance` | 谁依据什么接受或拒绝 | criteria snapshot, evidence refs, decision maker, decision | `kiana-domain` |
| `Delivery` | 什么产物被交付给谁 | artifact refs, version, channel, handoff, receipt | `kiana-domain` |
| `Outcome` | 交付后是否产生目标结果 | metric observations, target, realization, review date | `kiana-domain` |
| `AgentTemplate/Cell` | 谁执行、如何受限 | role, grant, budget, supervision, lineage | `kiana-domain` |
| `Session/Run/Invocation` | 一次执行如何恢复和对账 | owner, IDs, state, cursor, attempts | `kiana-domain` |
| `RunSnapshot` / `InvocationLedger` | 一次运行如何跨进程恢复、如何防重复副作用 | run_id, session_id, schema_version, step_counter, pending_capability, approval_decisions / `(run_id, call_id)`, args_fingerprint, status | `kiana-domain` |
| `DelegationPacket` | 父子 Cell 之间的运行时授权信封 | parent/child cell, capability/path scopes, budget/supervision lease, expires_at, delegation_allowed, max_turns, max_messages, termination_predicate, handoff_allowlist | `kiana-domain` |
| `RuntimeEvent/Artifact/Receipt` | 事实、产物和报告如何留存 | aggregate, version, provenance, refs | `kiana-domain` / `kiana-ports`（event ports）；runtime owner `kiana-eventlog`，Receipt 投影另见 `kiana-daemon` |

`Charter` 和 `Plan` 是 `Project` 的版本化工件，不单列为顶层聚合：Project 通过 `charter_ref` / `plan_ref` 引用它们的冻结版本，修改必须走 ChangeRequest 并产生新版本，不能原地覆盖。

`kiana-protocol` 负责这些对象的 versioned wire DTO；`kiana-core` 负责命令、授权和状态转移；`kiana-daemon` 负责组合、目录和投影。入口层不能成为领域对象的第二个 owner。

## 4. 最小领域合同

### 4.1 Objective

```text
Objective {
  objective_id
  organization_id
  parent_objective_id?
  title
  problem
  metric
  baseline
  target
  unit
  period_start
  period_end
  owner_principal_id
  priority
  status
  version
}
```

不变量：

1. `metric`、`target`、时间窗和 owner 必须存在；
2. Objective 只能由 Sponsor 或被授权的组织角色批准、暂停或放弃；
3. 完成 Project 不自动产生 `achieved`；必须满足下文的 Outcome 证据前置条件；
4. 目标变更必须留下版本和 DecisionRecord，不能覆盖历史目标。

目标状态：

```text
Proposed → Active / Rejected
Rejected → Archived
Active ↔ AtRisk
Active / AtRisk → Paused
Paused → Active / AtRisk
Active → Achieved
Active / AtRisk / Paused → Abandoned
Achieved / Abandoned → Archived
```

转移规则：

- `Proposed → Active` 必须由 Sponsor 或被授权的组织角色批准；
- `Active → Achieved` 的前置条件是至少一个关联 Outcome 为 `Realized`；`PartiallyRealized` 不满足该前置条件，必须留在 `Active` / `AtRisk` 继续测量，或由 Sponsor 通过 DecisionRecord 修订 target 或时间窗、产生新的 Objective 版本后重新判断；`NotRealized` 不得进入 `Achieved`；
- `Active / AtRisk / Paused → Abandoned` 必须记录 DecisionRecord 和原因；
- `Rejected`、`Achieved`、`Abandoned` 是终态；归档只改变可见性，不改变历史事实。

### 4.2 Initiative

```text
Initiative {
  initiative_id
  organization_id
  objective_refs[]
  title
  problem_statement
  hypothesis
  sponsor_id
  expected_value
  rough_cost
  risk_summary
  decision
  status
  version
}
```

目标状态：

```text
Intake → Triaged → Assessed → Approved
                         └────→ Rejected
Approved → ConvertedToProject → Closed
Rejected → Archived
```

`Approved` 只表示值得进入项目设计，不代表已经获得执行权限或外部支出权限。

### 4.3 Project

```text
Project {
  project_id
  organization_id
  portfolio_id?
  program_id?
  objective_refs[]
  sponsor_id
  charter_ref
  plan_ref?
  scope_baseline
  success_criteria[]
  non_goals[]
  workspace_ref?
  project_budget_ref?
  milestone_refs[]
  status
  version
}
```

项目状态：

```text
Proposed → Chartering → Approved → Planned → Active
Chartering → Rejected → Archived
Active ↔ Paused
Active ↔ AtRisk
Active ↔ ChangePending
Active / Paused / AtRisk / ChangePending → CancelRequested
CancelRequested → Cancelled / Active（撤回）
Active → ReadyForAcceptance
ReadyForAcceptance → Accepted → Closed → Archived
ReadyForAcceptance → Active（Rejected）
ReadyForAcceptance → Closed（Waived）
Active → Failed → Closed
Cancelled → Archived
```

关键规则：

- `Approved` 之前必须存在 Sponsor、目标、成功标准、非目标、风险初表和 go/no-go 决议；`Chartering → Rejected` 是 go/no-go 失败出口，必须记录 DecisionRecord，并只能 `→ Archived`；
- `Planned` 之前必须存在至少一个带冻结验收的 WorkPacket 或明确的“只读/无需派工”理由；
- `ChangePending` 期间不能偷偷改变旧的验收标准；
- `CancelRequested` 是中间态（wire 名 `cancel_requested`），必须收敛到 `Cancelled`，或经撤回回到取消请求前的可运行状态（至少 `Active`）；`cancelling` 只是 UI 投影名，不是状态名也不是终态；停止无法确认时必须进入 `result_unknown` 并关联 Incident，不得写成 `Cancelled`；
- `ReadyForAcceptance → Accepted` 由验收决策驱动；验收 `Rejected` 时回到 `Active` 返工，验收 `Waived` 时必须记录人工豁免 DecisionRecord 并可直接 `Closed`；两种去向不得互换；
- `Closed` 必须关联 Acceptance、Delivery、ClosingReceipt，或明确记录失败关闭和人工豁免；
- `Cancelled` 和 `Failed` 只能 `→ Archived`，不能复活；关闭后的重新工作必须创建新 ChangeRequest 或新 Project 版本，不能复活旧事实。

### 4.4 Milestone

```text
Milestone {
  milestone_id
  project_id
  objective_refs[]
  deliverables[]
  acceptance_criteria[]
  due_at
  dependency_refs[]
  status
  version
}
```

```text
Planned → Active
Active ↔ Blocked
Active → ReadyForAcceptance
Active / Blocked → Cancelled
ReadyForAcceptance → Accepted / Rejected
Rejected → Rework → ReadyForAcceptance
Accepted → Closed
```

`Rework` 表示 Milestone 被拒后，在冻结的 `acceptance_criteria` 内修正交付并重新提交验收的状态；Rework 期间不得修改验收标准，修改标准必须走 ChangeRequest 并产生新的 Milestone 版本。

Milestone 的 `Accepted` 只代表阶段交付符合标准，不等于顶层 Objective 已实现。

### 4.5 Acceptance

```text
Acceptance {
  acceptance_id
  project_id
  milestone_id?
  work_packet_id?
  criteria_snapshot
  evidence_refs[]
  reviewer_id
  decision_maker_id
  decision
  decided_at?
  rejection_reasons[]
  status
  version
}
```

```text
Requested → EvidencePending → ReadyForDecision
ReadyForDecision → Accepted / Rejected / Waived
Rejected → ReworkRequested → EvidencePending
Rejected → Closed
Accepted / Waived → Closed
```

- `criteria_snapshot` 在 Acceptance 进入 `Requested` 时从当时已冻结的 packet / milestone 标准派生并冻结（见 §4.9）；`EvidencePending` 和 `ReadyForDecision` 只读校验，不得改写。需要修改标准时必须走 ChangeRequest，作废旧 Acceptance 并新建 Acceptance；
- `Rejected → ReworkRequested` 与 `Rejected → Closed` 互斥：`ReworkRequested` 表示允许在冻结标准内返工并重新提交证据（回到 `EvidencePending`）；`Closed` 表示该验收请求终结、不再返工（后续变更必须走 ChangeRequest 或新 Milestone）。决策必须显式选择其一；一旦进入 `Closed`，不得再回到 `ReworkRequested`；
- `Waived` 必须记录豁免 DecisionRecord 和 authority chain；豁免不等于达标；
- Reviewer 可以拒绝或要求返工，但不能改写 Builder 的原始 Evidence、RuntimeEvent 或执行 Receipt。

### 4.6 ChangeRequest

```text
ChangeRequest {
  change_id
  project_id
  requested_by
  reason
  affected_scope[]
  affected_objectives[]
  affected_budget
  affected_schedule
  affected_risk
  proposed_baseline_version
  decision
  status
  version
}
```

```text
Draft → ImpactAssessed → PendingDecision
PendingDecision → Approved / Rejected
Rejected → Archived
Approved → Implementing → Verified → Closed → Archived
Implementing → Blocked / CancelRequested / Failed
Blocked → Implementing / CancelRequested / Failed
CancelRequested → Cancelled / Implementing
Failed → Archived
Cancelled → Archived
```

- `Blocked → Implementing` 是阻塞解除的出口；无法解除时必须显式 `Failed` 或 `CancelRequested`，不能停在 `Blocked`；
- `CancelRequested` 是中间态（wire 名 `cancel_requested`），必须收敛到 `Cancelled` 或经撤回回到 `Implementing`；
- `Rejected`、`Failed`、`Cancelled`、`Closed` 只能 `→ Archived`；重新变更必须新建 ChangeRequest。

每次批准的 ChangeRequest 都必须产生新的 scope、acceptance 或 budget baseline 版本。未经批准的变更只能作为提案，不能改变正在运行的 WorkPacket。

### 4.7 Risk 与 Incident

Risk 是尚未发生的可能性；Incident 是已经发生或无法排除的事实。二者不可混用。

```text
Risk {
  risk_id
  project_id
  description
  probability
  impact
  trigger
  mitigation
  contingency
  owner_id
  incident_id?
  status
}
```

```text
Identified → Assessed → Mitigating → Monitoring → Closed
Mitigating / Monitoring → Materialized → Closed
```

`Incident` 不是任何状态机的状态名，而是独立聚合：受影响聚合通过 `open_incident` 动作创建或关联 Incident，并回写 `incident_id?`。`Materialized` 必须已经关联 Incident；`Materialized → Closed` 必须引用 Incident 的验证证据。残余风险仍在时必须新建 Risk，不能复活旧记录。

```text
Incident {
  incident_id
  project_id?
  run_id?
  execution_id?
  risk_id?
  delivery_id?
  severity
  detected_at
  impact
  timeline[]
  owner_id
  response_actions[]
  evidence_refs[]
  escalation_target?
  status
}
```

```text
Open → Triaged → Assigned → Mitigating → Monitoring → Resolved → Closed
          └──────────────────────────────→ Escalated → Mitigating
```

以下情况必须通过 `open_incident` 创建或关联 Incident（并在源聚合回写 `incident_id`），而不能只返回普通错误：

- 发生了副作用但结果事件丢失；
- provider 或子进程状态无法确认；
- 取消返回但仍无法确认 handler 已停止；
- 事件、Artifact 或 Receipt 持久化失败导致事实不完整；
- 项目目标、预算、权限或数据边界发生未经批准的漂移。

### 4.8 Delivery 与 Outcome

```text
Delivery {
  delivery_id
  project_id
  acceptance_id
  artifact_refs[]
  version
  recipient_ref
  handoff_receipt_ref
  delivered_at
  incident_id?
  status
}
```

```text
Prepared → Approved → Delivered → Confirmed
Delivered → DeliveryUnknown
DeliveryUnknown → Reconciled → Confirmed / Failed
```

- `DeliveryUnknown` 不得自动重试或直接标 `Confirmed`；必须走 `reconcile_delivery`，或通过 `open_incident` 关联 Incident 并回写 `incident_id`；
- `Reconciled → Confirmed` 需要接收方确认证据；`Reconciled → Failed` 表示对账确认未交付，重试必须新建 Delivery 版本和新的 idempotency key，不能复用同一 `delivery_id`；
- `handoff_receipt_ref` 指向 `HandoffReceipt`，它证明接收方已 ACK，不等于业务结果已实现：

```text
HandoffReceipt {
  handoff_receipt_id
  delivery_id
  recipient_ref
  channel
  artifact_refs[]
  acknowledged_by
  acknowledged_at
  status: pending | acknowledged | rejected | unknown
}
```

```text
Outcome {
  outcome_id
  objective_id
  project_id
  metric_observations[]
  target_snapshot
  measurement_window
  owner_id
  review_at
  realization
  evidence_refs[]
  status
}
```

```text
Planned → Measuring → Realized / PartiallyRealized / NotRealized
```

`Realized` 是 `Active → Achieved` 的唯一充分证据（§4.1）；`PartiallyRealized` 和 `NotRealized` 都不满足该前置条件。

`Receipt` 证明 Kiana 记录了哪些事实；`Outcome` 才负责描述目标是否实现。Receipt 不能单独证明现实世界的业务结果。

### 4.9 验收标准四层

四层标准按下层不得宽于上层的方向派生：

```text
WorkPacket.acceptance_tests[]
  ⊆ Milestone.acceptance_criteria[]
  ⊆ Project.success_criteria[]
```

- `Project.success_criteria[]`：在 Charter go/no-go 批准（`Chartering → Approved`）时冻结；修改必须走 ChangeRequest 并产生新的 Charter / Project 版本；
- `Milestone.acceptance_criteria[]`：在 Milestone 进入 `Active`（`Planned → Active`）时冻结；修改必须走 ChangeRequest 并产生新的 Milestone 版本；
- `WorkPacket.acceptance_tests[]`：在 `approve_packet`（`PacketApproved`）时冻结；修改必须新建 packet 版本或走 ChangeRequest；
- `Acceptance.criteria_snapshot`：在 Acceptance 进入 `Requested` 时从上述已冻结版本派生并冻结（§4.5），是验收决策的唯一标准来源。

任何一层修改都必须留下版本和 DecisionRecord，不能覆盖历史标准。

### 4.10 运行恢复：RunSnapshot 与调用账本

`RunSnapshot` 是可序列化的 durable pause / resume 边界，由 ControlPlane 独占写入：

```text
RunSnapshot {
  run_id
  session_id
  schema_version
  step_counter
  messages[]                      // 规范化消息历史（role + 内容引用 + turn/step 序号）
  pending_capability? {
    call_id
    invocation_id
    args_fingerprint
  }
  approval_decisions[]            // ApprovalDecision 的 id 引用，见 §4.13
}
```

- runner 自身永远不读盘恢复；启动或首次访问某个 run 时，由 ControlPlane 调用 `resume_run` 从 `RunSnapshot` 重建 harness 状态。
- `args_fingerprint` 是对规范化后的 capability 类型 / 名称 / 参数做确定性哈希；续跑时指纹不一致必须立即拒绝，不得按漂移后的参数执行。
- `schema_version` 不兼容时 fail-closed，必须提供迁移或 upcaster，不得猜测旧结构。
- 跨进程恢复默认暂停（fail-closed）：重启后只从持久事实重建待审批列表与运行态，不自动续跑；必须由用户显式「恢复」后才继续。重建出的 pending capability 必须重新过 policy / gate / approval，不能直接交给 broker。

调用账本由 ControlPlane 独占写入，键唯一：

```text
InvocationLedgerEntry {
  run_id
  call_id
  invocation_id
  attempt
  args_fingerprint
  status          // executed | unknown | rejected
  result_ref?     // 已执行结果的缓存引用
}
```

- 键为 `(run_id, call_id)`；派发任何 capability 前先查账本：已 `executed` 直接返回缓存结果，`args_fingerprint` 不一致立即拒绝，`unknown` 不得自动重试、必须走 reconciliation 并视情况关联 Incident（§4.7）。
- 账本是事实而不是缓存；重复命令不得制造第二个副作用。

### 4.11 任务图：依赖不变量与 claim

`WorkPacket.dependencies[]` 是显式依赖边，只从字段读取，不从 packet 文本解析。就绪只有一个定义：

```text
ready_packets(graph, now) =
      status ∈ 可派发集合
    ∧ 所有 dependencies 处于成功终态
    ∧ 不存在未过期 lease 冲突（now ≥ lease_expires_at 视为过期）
```

- `completed` / `accepted` 属于成功终态；`failed` / `cancelled` / `result_unknown` 不是。ControlPlane 的 spawn 校验、`kiana project next` 与看板必须都调用同一个 `ready_packets`，禁止各写一份。
- spawn 前校验依赖：未满足时返回 `blocked` 并记事件，不得把 Draft → Approved → Assigned → Running 直接推完。
- `blocked` 是依赖图的不动点：任一 packet 处于 blocked，其子 packet 也 blocked（父子继承），递归折叠到不再变化为止。
- `validate_dependency_dag` 做确定性环检测：输出规范化环（节点按稳定 id 排序），两次运行字节一致；在 `approve_packet` 与 workflow 模板注册时调用，检测到环即拒绝落盘。
- `WorkPacket` 增加派生 claim：

```text
claim? {
  owner              // 认领者 cell_id
  lease_expires_at
  heartbeat_at
}
```

- spawn / continue / 每个 turn 续租 `heartbeat_at`；后台确定性扫描过期 lease，把 packet 退回 ready 并记事件。默认 TTL 与扫描周期由 policy 给定。
- `ready_packets` 只是查询；真正的执行许可仍由 ControlPlane 的 policy / gates / approval 产生。`ready_packets` 与 PathLock 的关系固定为「规划期检查 + 运行期兜底」。

### 4.12 委派信封与失败合并

`DelegationPacket` 的基础合同见 [`company-os-design.md`](company-os-design.md) §6.2；本条只做增量。委派是 ControlPlane 的 assign / handoff 操作，不是模型可见工具：模型只能在 WorkPacket 内容里提出建议，实际目标由 ControlPlane 按 role / grant 白名单解析。

```text
DelegationPacket 增量 {
  max_turns
  max_messages
  termination_predicate       // AND / OR 组合的显式谓词，不是模型文本
  handoff_allowlist[]         // 只来自 grant / template，不来自模型文本
}
```

- 超出 `max_turns` / `max_messages` 自动终止并写事件；`handoff_allowlist` 之外的 target 一律拒绝。
- 子 Cell 失败以 typed payload 随 EventLog 持久化：

```text
ChildFailureReport {
  child_cell_id
  reason_code
  error_class
  partial_output_refs[]
  result_unknown
  retryable
  policy_snapshot
}
```

- `MergeDecision` 是父 Cell 对子输出的合并裁决；`result_unknown = true` 的 child 输出一律拒绝合并，不得当成成功：

```text
MergeDecision {
  parent_cell_id
  child_cell_id
  decision          // merged | rejected | superseded
  basis_refs[]
  rejected_reason?
}
```

- 委派生命周期事件为 `delegation_started` / `delegation_completed` / `delegation_failed` / `delegation_reconciled`，必须带 parent / child 关联。

### 4.13 审批决定

审批决定是一条 durable、用户可见的事件卡，由 ControlPlane 追加进 EventLog 并投影到 transcript 和 Receipt：

```text
ApprovalDecision {
  decision_id
  approval_id
  actor              // user / reviewer；自动批准标记为 system:auto
  scope              // once / turn / session / policy
  subject            // 精确目标：命令 / 路径 / host
  expiry_at?
  result             // consumed / denied / expired / cancelled
  feedback?          // 拒绝理由，作为带 feedback 的 tool result 回灌模型
  decided_at
}
```

- `denied`（拒绝但继续）与 `cancelled`（拒绝并中止）是不同终态，不得合并；`expired` 与 `consumed` 也不同。拒绝只拒绝当前 invocation，`cancelled` 才是终止 run。
- 首发决定集为「批准这一次 / 拒绝并继续 / 拒绝并中止」三个动作；「本会话批准」与「持久 prefix 规则」列为第二阶段开放决策，不在首发目标内。
- 自动批准只在 LocalWrite 一档、且开关默认关闭时才可能发生；每次自动批准必须追加带「自动批准」标记的 `ApprovalDecision`，在 Receipt 里可见，更高风险一律弹审批。
- `scope` 的完整四级阶梯（once / turn / session / policy）是目标合同；首发只落地与上述三个动作对应的作用域（`once` 及拒绝路径），`turn` / `session` / `policy` 三级按 A-4 的阶梯逐步落地，其中 `session` / `policy` 的持久化与撤销列为第二阶段开放决策。

## 5. 执行事实与公司事实的连接

### 5.1 关联规则

每个 Runtime 请求应尽可能携带：

```text
organization_id
project_id?
objective_refs[]
milestone_id?
work_packet_id?
cell_id?
run_id
turn_id
invocation_id?
acceptance_id?
```

缺少 `project_id` 的 L0/L1 本地动作可以作为 standalone run，但不能伪装成某个 Project 的交付证据。

### 5.2 正式交付链

```text
Sponsor
  → Objective
  → Initiative / Project
  → Charter + Success Criteria
  → Milestone + WorkPacket
  → Builder Cell + Run
  → CapabilityExecution + Evidence
  → Independent Review
  → Acceptance
  → Delivery
  → ClosingReceipt
  → Outcome measurement
```

每个箭头都必须有：

- 来源对象和目标对象的 ID；
- 产生或消费的事件；
- owner 和 authority chain；
- 允许的状态转移；
- 失败、取消、返工和 Unknown 语义。

### 5.3 预算分层

```text
FinancialBudget
  └── ProjectBudget
        └── RuntimeBudget          Run 级预算
              └── BudgetLease      Cell 级派生租约
```

- `FinancialBudget`：支付、收入、现金或合同承诺；当前不实现；
- `ProjectBudget`：项目的人力、时间、资源和成本基线；目标对象；
- `RuntimeBudget`：一个 Run 的 token、工具、墙钟和存储限额；
- `BudgetLease`：由父级 `RuntimeBudget` 派生的 Cell 级租约，只能更窄，不能 union 或提升；预算命名以 `BudgetLease` 为 canonical。

子 Cell 只能在父级有效租约和项目授权的交集内获得更窄的 BudgetLease。运行时节省预算不自动代表项目实现了收益。`BudgetLease` 当前局部实现，`RuntimeBudget` 和 `ProjectBudget` 仍是目标对象。

## 6. 命令、事件和证据规则

### 6.1 命令边界

| 聚合 | 命令 | 发起者 | 必须前置条件 | 成功事件 | 状态转移 |
|---|---|---|---|---|---|
| Objective | `propose_objective` | Sponsor | 组织身份有效；metric、target、时间窗和 owner 完整 | `ObjectiveProposed` | → Proposed |
| Objective | `approve_objective` / `reject_objective` | Sponsor 或授权组织角色 | Proposed | `ObjectiveActivated` / `ObjectiveRejected` | Proposed → Active / Rejected |
| Objective | `mark_objective_at_risk` / `clear_objective_risk` | owner / Sponsor | Active / AtRisk | `ObjectiveAtRisk` / `ObjectiveRiskCleared` | Active ↔ AtRisk |
| Objective | `pause_objective` / `resume_objective` | Sponsor 或授权组织角色 | Active/AtRisk / Paused | `ObjectivePaused` / `ObjectiveResumed` | Active/AtRisk ↔ Paused |
| Objective | `achieve_objective` | Sponsor / outcome owner | 至少一个关联 Outcome 为 `Realized` | `ObjectiveAchieved` | Active → Achieved |
| Objective | `abandon_objective` | Sponsor 或授权组织角色 | Active/AtRisk/Paused；DecisionRecord 完整 | `ObjectiveAbandoned` | → Abandoned |
| Objective | `archive_objective` | Sponsor / Closer | Rejected / Achieved / Abandoned | `ObjectiveArchived` | → Archived |
| Initiative | `submit_initiative` | 任一 principal | problem、hypothesis、sponsor 存在 | `InitiativeSubmitted` | → Intake |
| Initiative | `triage_initiative` / `assess_initiative` | Planning | Intake；价值、成本、风险初评完整 | `InitiativeTriaged` / `InitiativeAssessed` | Intake → Triaged → Assessed |
| Initiative | `decide_initiative` | Sponsor | Assessed | `InitiativeApproved` / `InitiativeRejected` | Assessed → Approved / Rejected |
| Initiative | `convert_initiative` | Sponsor | Approved | `InitiativeConverted` | Approved → ConvertedToProject |
| Initiative | `close_initiative` / `archive_initiative` | Planning | ConvertedToProject / Rejected | `InitiativeClosed` / `InitiativeArchived` | → Closed / Archived |
| Project | `propose_project` / `start_chartering` | Sponsor / Planning | objective_refs 有效 / Proposed | `ProjectProposed` / `ProjectCharteringStarted` | → Proposed → Chartering |
| Project | `approve_project` / `reject_project` | Sponsor / authorized approver | Charter、目标、成功标准、非目标、风险初表、预算和 go/no-go 决议完整 | `ProjectApproved` / `ProjectRejected` | Chartering → Approved / Rejected |
| Project | `plan_project` / `activate_project` | Planning / ControlPlane | 至少一个带冻结验收的 WorkPacket 或只读理由；grant、budget、依赖有效 | `ProjectPlanned` / `ProjectActivated` | Approved → Planned → Active |
| Project | `pause_project` / `resume_project` | Sponsor / Planning | Active/AtRisk/ChangePending / Paused | `ProjectPaused` / `ProjectResumed` | ↔ Paused |
| Project | `mark_project_at_risk` / `clear_project_risk` | owner / Sponsor | Active / AtRisk | `ProjectAtRisk` / `ProjectRiskCleared` | ↔ AtRisk |
| Project | `open_change` | owner / Sponsor | Active；影响和原因已记录 | `ProjectChangePending` | Active → ChangePending；由 ChangeRequest 的 `decide_change` 决定后回到 Active / Paused |
| Project | `request_cancel_project` / `withdraw_cancel_project` / `confirm_cancel_project` | Sponsor / owner | 请求：Active/Paused/AtRisk/ChangePending；撤回：CancelRequested；确认：已确认停止或明确记录 `result_unknown` | `ProjectCancelRequested` / `ProjectCancelWithdrawn` / `ProjectCancelled` | → CancelRequested → 请求前状态 / Cancelled |
| Project | `request_acceptance` | Builder / Closer | Active；Evidence 已持久化 | `AcceptanceRequested` | Active → ReadyForAcceptance |
| Project | `decide_acceptance` | 独立 Reviewer / Sponsor | ReadyForAcceptance；criteria snapshot 和 Evidence 可核验 | `AcceptanceDecided` | ReadyForAcceptance → Accepted / Active（Rejected）/ Closed（Waived） |
| Project | `fail_project` / `close_project` / `archive_project` | Closer / Sponsor | 失败原因和证据；Acceptance、Delivery、Receipt 或失败豁免完整 | `ProjectFailed` / `ProjectClosed` / `ProjectArchived` | Active → Failed → Closed → Archived |
| Milestone | `create_milestone` | Planning | Project 已批准 | `MilestoneCreated` | → Planned |
| Milestone | `activate_milestone` / `block_milestone` / `unblock_milestone` | Planning | Planned / Active / Blocked | `MilestoneActivated` / `MilestoneBlocked` / `MilestoneUnblocked` | Planned → Active ↔ Blocked |
| Milestone | `submit_milestone_acceptance` | Builder / Closer | Active；deliverables 和 Evidence 完整 | `MilestoneAcceptanceRequested` | Active → ReadyForAcceptance |
| Milestone | `decide_milestone` | 独立 Reviewer / Sponsor | ReadyForAcceptance；criteria snapshot 可核验 | `MilestoneAccepted` / `MilestoneRejected` | ReadyForAcceptance → Accepted / Rejected |
| Milestone | `rework_milestone` / `resubmit_milestone` | owner | Rejected / Rework 完成 | `MilestoneReworkStarted` / `MilestoneResubmitted` | Rejected → Rework → ReadyForAcceptance |
| Milestone | `cancel_milestone` / `close_milestone` | Planning / owner | Active/Blocked / Accepted | `MilestoneCancelled` / `MilestoneClosed` | → Cancelled / Closed |
| WorkPacket | `approve_packet` / `accept_packet` | Planning / 指定 acceptor | 写集、验收、预算、owner 存在；packet 内容未漂移 | `PacketApproved` / `PacketAccepted` | 见 `company-os-design.md` §9.2 |
| WorkPacket | `claim_packet` / `renew_packet_lease` / `reclaim_packet` | ControlPlane | 依赖全部成功终态；无未过期 lease 冲突 | `PacketClaimed` / `PacketLeaseRenewed` / `PacketReclaimed` | 见 §4.11 |
| Approval | `decide_approval` | user / reviewer | pending approval 存在；决定属于首发决定集 | `ApprovalDecided` | → consumed / denied / expired / cancelled（见 §4.13） |
| Delegation | `start_delegation` / `complete_delegation` / `fail_delegation` / `reconcile_delegation` / `merge_delegation` | ControlPlane | grant、预算有效；目标在 `handoff_allowlist` 内 | `DelegationStarted` / `DelegationCompleted` / `DelegationFailed` / `DelegationReconciled` / `MergeDecided` | 见 §4.12 |
| Run | `start_run` | ControlPlane | packet、grant、budget 和依赖有效 | `RunStarted` | 见 `company-os-platform-architecture.md` |
| Acceptance | `request_acceptance` / `decide_acceptance` | Builder / Closer；独立 Reviewer / Sponsor | 见 §4.5 | `AcceptanceRequested` / `AcceptanceDecided` | 见 §4.5 |
| ChangeRequest | `request_change` / `decide_change` | owner / Sponsor | 说明影响和原因；impact assessment 完整 | `ChangeRequested` / `ChangeApproved` / `ChangeRejected` | Draft → ImpactAssessed → PendingDecision → Approved / Rejected |
| ChangeRequest | `implement_change` / `block_change` / `unblock_change` / `verify_change` / `cancel_change` / `close_change` / `archive_change` | owner / Sponsor | 见 §4.6 | `ChangeImplemented` / `ChangeBlocked` / `ChangeUnblocked` / `ChangeVerified` / `ChangeCancelRequested` / `ChangeCancelled` / `ChangeClosed` / `ChangeArchived` | 见 §4.6 |
| Risk | `identify_risk` / `assess_risk` / `mitigate_risk` / `monitor_risk` / `close_risk` | owner / Sponsor | 见 §4.7 | `RiskIdentified` / `RiskAssessed` / `RiskMitigationStarted` / `RiskMonitoring` / `RiskClosed` | Identified → Assessed → Mitigating → Monitoring → Closed |
| Risk | `materialize_risk` + `open_incident` | owner / Sponsor | 触发条件成立 | `RiskMaterialized` / `IncidentOpened` | → Materialized；回写 `incident_id` |
| Incident | `open_incident` / `triage_incident` / `assign_incident` / `mitigate_incident` / `monitor_incident` / `escalate_incident` / `resolve_incident` / `close_incident` | owner / on-call / Sponsor | 见 §4.7 | `IncidentOpened` / `IncidentTriaged` / `IncidentAssigned` / `IncidentMitigationStarted` / `IncidentMonitoring` / `IncidentEscalated` / `IncidentResolved` / `IncidentClosed` | 见 §4.7 |
| Delivery | `prepare_delivery` / `approve_delivery` / `deliver` / `confirm_delivery` | Closer / recipient | 见 §4.8 | `DeliveryPrepared` / `DeliveryApproved` / `Delivered` / `DeliveryConfirmed` | Prepared → Approved → Delivered → Confirmed |
| Delivery | `mark_delivery_unknown` / `reconcile_delivery` | ControlPlane / Closer | 结果无法确认 / 对账证据完整 | `DeliveryUnknown` / `DeliveryReconciled` | Delivered → DeliveryUnknown → Reconciled → Confirmed / Failed |
| Outcome | `plan_outcome` / `start_measurement` / `record_outcome` | Closer / outcome owner | measurement window 和 evidence 存在 | `OutcomePlanned` / `OutcomeMeasuring` / `OutcomeRecorded` | Planned → Measuring → Realized / PartiallyRealized / NotRealized |

模型文本、普通 Chat、网页内容和 Agent 自报不能直接执行上述命令。

最小命令集与补齐顺序：先补齐 Objective / Project / Milestone / Acceptance / Delivery / Outcome（对应 §9 的单项目闭环），再补齐 ChangeRequest / Risk / Incident 以及运行侧 cancel / incident 命令；未列入本表的转移不得在实现中自创命令名。

### 6.2 事件不变量

所有事件遵守 [`company-os-design.md`](company-os-design.md) §11 的最低字段和 Event Store 规则；本文不另立完整清单，只增量追加 Company 业务链的关联字段：

```text
project_id?
milestone_id?
acceptance_id?
delivery_id?
outcome_id?
change_id?
risk_id?
incident_id?
run_id?
invocation_id?
call_id?
delegation_id?
```

这些字段只做关联，不替代 design §11 的字段，也不把执行事实升级为公司事实。

此外：

1. 事件的 aggregate stream 必须支持 expected version/CAS；
2. 重复命令必须返回同一逻辑结果或明确拒绝 payload 漂移；
3. 状态投影失败不能删除或覆盖已提交事实；
4. Receipt 只能从事件、Artifact 和独立验证结果投影；
5. 历史事件不可被 Review、Change 或自然语言总结改写；
6. schema major 不兼容时必须 fail-closed，并提供迁移或 upcaster。

### 6.3 Receipt 最低回答

任何 Project 级 ClosingReceipt 至少回答：

```text
why         哪个 Objective / Initiative 触发
what        哪个 Project / Milestone / WorkPacket
who         Sponsor、owner、acceptor、reviewer
allowed     哪些 Grant、Budget、Approval 生效
changed     哪些文件、Artifact 或外部资源改变
verified    哪些测试、Review 和 Acceptance 完成
exceptions  哪些 deny、retry、cancel、rework、Unknown、Incident
result      交付事实与 Outcome 测量是否分开
next        后续维护、对账或新 Initiative
```

## 7. 最小 CompanyOS 纵向切片

第一条完整业务路径不需要实现所有部门或所有外部能力，只需完成一个本地 Coding 项目：

```text
Sponsor 提交 Objective
  → 生成 Project Charter
  → Sponsor 批准 Project
  → Planning 创建 Milestone + WorkPacket
  → Builder 在锁定路径运行
  → EventLog 持久化 Run / Invocation / Evidence
  → 独立 Reviewer 依据冻结 criteria 审查
  → Sponsor / acceptor 作 Acceptance 决策
  → Closer 生成 Delivery + ClosingReceipt
  → 在测量窗口记录 Outcome
```

必须同时覆盖四条负向路径：

1. 未批准 Project 或 WorkPacket：不得启动写操作；
2. 审批拒绝或过期：不得继续 Invocation；
3. 取消或崩溃后：不得错误产生 `completed`；
4. 结果 Unknown：不得盲目重试，必须进入 Incident/Reconciliation。

这一切片的完成标准不是“Agent 数量增加”，而是一个新的进程可以依据持久事实重建：

```text
Objective → Project → Packet → Run → Evidence → Acceptance → Receipt
```

## 8. 与现有文档和代码的关系

| 现有材料 | 本文如何使用 |
|---|---|
| `docs/company-os-design.md` | 继续作为总体产品、PMP、能力风险、Cell、Approval、Event 和安全方向的规范来源 |
| `docs/company-os-security-constitution.md` | 继续作为 deny、cancel、Unknown、secret、TOCTOU 和外部副作用的强制边界 |
| `docs/company-os-implementation-outline.md` | 增加本合同的 schema、状态机、投影和纵向切片实施卡 |
| `docs/company-os-spec-index.md` | 索引本文，并把 Company domain 与 Runtime domain 区分开 |
| `COMPANY.md` | 保留部门、角色和组织心智模型；不重复本文的字段合同 |
| `CURRENT_STATUS.md` | 继续决定当前状态和证明等级；本文目标对象不能提升当前声明 |
| `kiana-tasks` | 只作为兼容 adapter 或调度 projection；canonical WorkPacket 不迁回旧类型 |
| `kiana-domain` | 承载稳定 ID、对象、值对象、不变量和状态转移 |
| `kiana-protocol` | 承载 versioned DTO、命令、事件和错误 wire contract |
| `kiana-core` | 承载命令处理、authority chain、状态转移和 ControlPlane policy |
| `kiana-eventlog` / `kiana-ports` | 承载事实追加、CAS、恢复、State/Artifact/Receipt ports |
| `kiana-daemon` | 承载组合根、目录、投影、调度和本地 adapter |

## 9. 实施顺序与发布门

全局阶段序列以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 为唯一 canonical；本节只登记领域合同在各阶段的落地顺序和退出条件，不定义平行的阶段编号。

### 领域合同收敛（spec-index §7 P0）

- 为本文每个聚合（含 `Charter`、`Plan`）指定 schema/version、canonical owner 和 runtime owner；
- 把 `implemented / partial / target / deferred / not_supported` 与 `source / local_behavior / durable / live / physical` 分开；
- 统一 README、DESIGN、PHASES、COMPANY、CURRENT_STATUS 的版本和状态叙事；
- 按 [`schemas/README.md`](schemas/README.md) §3 的目标目录建立 domain schema registry（`docs/schemas/domain/*.v1.schema.json`）；
- 建立每个聚合的命令、事件、错误、Receipt 和迁移表（§6.1）。

### 可靠的单项目闭环（spec-index §7 P3）

- 先完成 Session/Run/Invocation 的 durable ledger、RunSnapshot 与 `(run_id, call_id)` 调用账本、Approval continuation（含 `ApprovalDecision` 事件）、State projection 和 Receipt rebuild；
- 实现 Objective、Project、Milestone、Acceptance、Delivery、Outcome 的最小 domain contracts；
- 以一个 fake-model Coding 项目证明四条负向路径和一条成功路径；
- Project、Packet、Review 和 Acceptance 的 owner、acceptor、reviewer 不得由客户端自报覆盖。

### 运营治理（spec-index §7 P2 → P3）

- 加入 ChangeRequest、Risk、Incident、Escalation、Rework 和 Delivery reconciliation 的命令与事件（§6.1）；
- 为暂停、恢复、取消、失败关闭、撤回取消和重新立项定义命令与事件；
- 形成项目级 dashboard/query projection，但 UI 仍只是 State/Receipt 的投影；
- 加入 Outcome measurement 和 lessons 写回，而不是把总结文本直接当成结果。

### 外部 bounded context（spec-index §7 P5）

只有上述阶段连续通过后，才选择一个低风险域（例如 Office 草稿）建立独立 adapter。外部 Commerce、Travel、Payment、IoT 必须额外具备：

```text
Provider account identity
→ exact approval
→ idempotency key
→ provider receipt
→ reconciliation
→ cancellation / refund / incident
```

它们不能共享 Coding 的宽泛 shell 权限，也不能把 `BudgetLease` 当作金融授权。

## 10. 目标完成定义

在 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 出现绑定这些对象的证据块、并把相应条目提升到 `local_behavior` 之前，CompanyOS 领域合同至少必须满足：

- 目标可以追踪到项目、里程碑、工作包、证据、验收和 Receipt；
- Project 状态可以由事件重建，关闭不依赖内存中的全局 active；
- 任何范围或验收变化都有 ChangeRequest 和新版本；
- Review 不会改变作者原始事实，Acceptance 有独立决策者；
- 取消、崩溃、持久化失败和 Unknown 都有明确终态或 Incident；取消中间态 `cancel_requested` 必须收敛，`cancelling` 只作为 UI 投影名，未知终态只有 `result_unknown`；
- 每台状态机的每条转移都收敛到明确终态或归档出口，不存在死端；
- 重复命令、重复审批和重复交付不会制造未授权的重复副作用；
- RuntimeBudget、BudgetLease、ProjectBudget 和 FinancialBudget 不会混为一谈；
- Outcome 的“业务结果”不会由模型文字或普通 Receipt 自动宣称；
- 运行态可由 `RunSnapshot` 与 `(run_id, call_id)` 调用账本重建，跨进程恢复默认暂停、显式恢复后才续跑，重复 call_id 不产生第二个副作用；
- 任务图的「就绪」只有 `ready_packets` 一个定义，依赖缺失或成环在启动前 fail-closed，过期 lease 可被确定性回收；
- 委派不新增模型可见工具，`result_unknown` 的子输出不得进入 `MergeDecision`；
- 新的 CLI、Workbench、Web 入口都复用同一个 ControlPlane 和事实源。

在这些条件之前，Kiana 的诚实描述仍是：

> **Kiana 具备本地 Agent 执行治理和 Coding 交付的控制面骨架；完整的 CompanyOS 业务生命周期、持久化恢复和外部业务域仍处于分阶段建设中。**
