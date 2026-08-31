# Kiana CompanyOS 领域合同与业务生命周期

> 文档性质：规范目标与领域合同（Normative Target）。
>
> 本文补齐 CompanyOS 的“Company”一侧：目标、项目、交付、验收、运营和结果。它不替代 `company-os-design.md` 的总体原则、`company-os-security-constitution.md` 的安全宪法或 `company-os-implementation-outline.md` 的工程切片。
>
> 当前 checkout 的能力上限仍以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文中的对象和状态机未被实现或测试证明时，不得写成当前能力。

> **本文速览（导读，非规范）**
>
> - **讲什么**："公司业务"一侧的对象合同——从目标（Objective）、立项（Initiative）、项目（Project）、里程碑（Milestone），到验收（Acceptance）、交付（Delivery）、结果测量（Outcome），以及变更（ChangeRequest）、风险（Risk）和事故（Incident）的字段、状态机和规则。
> - **回答的问题**："一件工作为什么值得做、做到什么算完、谁说了算、交付之后目标到底实现没实现。"
> - **核心思想**：执行事实（Agent 跑成功了）和公司事实（业务目标实现了）是两条链，前者不能自动推出后者。
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

以下目录是目标合同。对象字段可随 schema 版本演进，但不能在不同 crate 或文档中重新定义同名语义。

| 聚合 | 解决的问题 | 最小关键字段 | canonical owner |
|---|---|---|---|
| `Organization` | 谁拥有目标、策略和资源 | `organization_id`, owner, policy profile, status | `kiana-domain` |
| `Objective` | 为什么做、什么算结果 | metric, baseline, target, period, owner | `kiana-domain` |
| `Portfolio` | 如何在多个目标间投资 | objectives, projects, priority, budget baseline | `kiana-domain` |
| `Program` | 哪些项目共享依赖或风险 | projects, dependencies, risk owner | `kiana-domain` |
| `Initiative` | 想法如何进入立项 | problem, hypothesis, sponsor, score, decision | `kiana-domain` |
| `Project` | 一个交付单元的生命周期 | charter, scope, baseline, milestones, sponsor, status | `kiana-domain` |
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
| `RuntimeEvent/Artifact/Receipt` | 事实、产物和报告如何留存 | aggregate, version, provenance, refs | `kiana-eventlog` + ports |

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
3. 完成 Project 不自动产生 `achieved`；必须有 Outcome 的观测证据；
4. 目标变更必须留下版本和 DecisionRecord，不能覆盖历史目标。

目标状态：

```text
Proposed → Active → AtRisk → Achieved
                    └──────→ Abandoned
Active / Achieved / Abandoned → Archived
```

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
Active → Paused / AtRisk / ChangePending / CancelRequested
Paused → Active / Cancelled
AtRisk → Active / Paused / Cancelled
ChangePending → Active / Paused / Cancelled
Active → ReadyForAcceptance → Accepted → Closed → Archived
Active → Failed → Closed
```

关键规则：

- `Approved` 之前必须存在 Sponsor、目标、成功标准、非目标、风险初表和 go/no-go 决议；
- `Planned` 之前必须存在至少一个带冻结验收的 WorkPacket 或明确的“只读/无需派工”理由；
- `ChangePending` 期间不能偷偷改变旧的验收标准；
- `Closed` 必须关联 Acceptance、Delivery、ClosingReceipt，或明确记录失败关闭和人工豁免；
- 关闭后的重新工作必须创建新 ChangeRequest 或新 Project 版本，不能复活旧事实。

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
Planned → Active → Blocked → ReadyForAcceptance
                    └──────→ Cancelled
ReadyForAcceptance → Accepted / Rejected
Rejected → Rework → ReadyForAcceptance
Accepted → Closed
```

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
Accepted / Waived / Rejected → Closed
```

`criteria_snapshot` 必须是验收时的冻结版本。Reviewer 可以拒绝或要求返工，但不能改写 Builder 的原始 Evidence、RuntimeEvent 或执行 Receipt。

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
Approved → Implementing → Verified → Closed
Implementing → Blocked / CancelRequested / Failed
```

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
  status
}
```

```text
Identified → Assessed → Mitigating → Monitoring → Closed
                             └──────→ Materialized → Incident
```

```text
Incident {
  incident_id
  project_id?
  run_id?
  execution_id?
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

以下情况必须创建或关联 Incident，而不能只返回普通错误：

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
  status
}
```

```text
Prepared → Approved → Delivered → Confirmed
                          └──────→ DeliveryUnknown
DeliveryUnknown → Reconciled / Incident
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

`Receipt` 证明 Kiana 记录了哪些事实；`Outcome` 才负责描述目标是否实现。Receipt 不能单独证明现实世界的业务结果。

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
        └── RuntimeBudget / BudgetLease
```

- `FinancialBudget`：支付、收入、现金或合同承诺；当前不实现；
- `ProjectBudget`：项目的人力、时间、资源和成本基线；目标对象；
- `BudgetLease`：某个 Cell/Run 的 token、工具、墙钟、并发和 effect 配额；当前局部实现。

子 Cell 只能在父级有效租约和项目授权的交集内获得更窄的 BudgetLease。运行时节省预算不自动代表项目实现了收益。

## 6. 命令、事件和证据规则

### 6.1 命令边界

| 命令 | 发起者 | 必须前置条件 | 成功结果 |
|---|---|---|---|
| `propose_objective` | Sponsor | 组织身份有效 | `ObjectiveProposed` |
| `approve_project` | Sponsor / authorized approver | Charter、目标、风险、预算存在 | `ProjectApproved` |
| `create_milestone` | Planning | Project 已批准 | `MilestoneCreated` |
| `approve_packet` | Planning | 写集、验收、预算、owner 存在 | `PacketApproved` |
| `accept_packet` | 指定 acceptor | packet 内容未漂移 | `PacketAccepted` |
| `start_run` | ControlPlane | packet、grant、budget 和依赖有效 | `RunStarted` |
| `request_change` | owner / Sponsor | 说明影响和原因 | `ChangeRequested` |
| `decide_change` | Sponsor / authorized approver | impact assessment 完整 | `ChangeApproved` 或 `ChangeRejected` |
| `request_acceptance` | Builder / Closer | Evidence 已持久化 | `AcceptanceRequested` |
| `decide_acceptance` | 独立 Reviewer / Sponsor | criteria snapshot 和 Evidence 可核验 | `AcceptanceDecided` |
| `close_project` | Closer / Sponsor | Acceptance、Delivery、Receipt 或失败豁免完整 | `ProjectClosed` |
| `record_outcome` | Closer / outcome owner | measurement window 和 evidence 存在 | `OutcomeRecorded` |

模型文本、普通 Chat、网页内容和 Agent 自报不能直接执行上述命令。

### 6.2 事件不变量

所有事件继续遵守 `company-os-design.md` 中的最低字段和 Event Store 规则：

```text
event_id
aggregate_type
aggregate_id
stream_version
correlation_id
causation_id
request_id
session_id?
run_id?
turn_id?
actor_id
organization_id
project_id?
idempotency_key
occurred_at
schema_version
payload
```

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

### P0：规范收敛

- 为本文每个聚合指定 schema/version、canonical owner 和 runtime owner；
- 把 `implemented / partial / target / deferred / not_supported` 与 `source / local_behavior / durable / live / physical` 分开；
- 统一 README、DESIGN、PHASES、COMPANY、CURRENT_STATUS 的版本和状态叙事；
- 在 `docs/schemas/company-os/` 建立 domain schema registry；
- 建立每个聚合的命令、事件、错误、Receipt 和迁移表。

### P1：可靠的单项目闭环

- 先完成 Session/Run/Invocation 的 durable ledger、Approval continuation、State projection 和 Receipt rebuild；
- 实现 Objective、Project、Milestone、Acceptance、Outcome 的最小 domain contracts；
- 以一个 fake-model Coding 项目证明四条负向路径和一条成功路径；
- Project、Packet、Review 和 Acceptance 的 owner、acceptor、reviewer 不得由客户端自报覆盖。

### P2：运营治理

- 加入 ChangeRequest、Risk、Incident、Escalation、Rework 和 Delivery；
- 为暂停、恢复、取消、失败关闭和重新立项定义命令与事件；
- 形成项目级 dashboard/query projection，但 UI 仍只是 State/Receipt 的投影；
- 加入 Outcome measurement 和 lessons 写回，而不是把总结文本直接当成结果。

### P3：独立外部 bounded context

只有 P0–P2 连续通过后，才选择一个低风险域（例如 Office 草稿）建立独立 adapter。外部 Commerce、Travel、Payment、IoT 必须额外具备：

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

CompanyOS 领域合同达到 `proven_local` 前，至少必须满足：

- 目标可以追踪到项目、里程碑、工作包、证据、验收和 Receipt；
- Project 状态可以由事件重建，关闭不依赖内存中的全局 active；
- 任何范围或验收变化都有 ChangeRequest 和新版本；
- Review 不会改变作者原始事实，Acceptance 有独立决策者；
- 取消、崩溃、持久化失败和 Unknown 都有明确终态或 Incident；
- 重复命令、重复审批和重复交付不会制造未授权的重复副作用；
- RuntimeBudget、ProjectBudget 和 FinancialBudget 不会混为一谈；
- Outcome 的“业务结果”不会由模型文字或普通 Receipt 自动宣称；
- 新的 CLI、Workbench、Web 入口都复用同一个 ControlPlane 和事实源。

在这些条件之前，Kiana 的诚实描述仍是：

> **Kiana 具备本地 Agent 执行治理和 Coding 交付的控制面骨架；完整的 CompanyOS 业务生命周期、持久化恢复和外部业务域仍处于分阶段建设中。**
