# Kiana CompanyOS 运营、治理与可靠性

> 文档性质：运营与治理规范目标（Normative Target）。
>
> 本文补齐 CompanyOS 在 Agent Platform 之外的运行保障：身份与组织管理、触发器与调度、人类决策入口、Artifact/Workspace 交付、成本与容量、可靠性、数据治理和集成边界。
>
> 当前实现和证据等级以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准。本文中的对象、状态机和参考设计尚未被实现或测试证明时，不得写成当前能力。

> **本文速览（导读，非规范）**
>
> - **讲什么**：让系统"长期跑而不出事"的运营保障——身份与授权（Principal/Membership/RoleAssignment/SharingGrant）、定时与触发（Trigger/Scheduler）、人类决策收件箱（Human Inbox；HumanTask 是 Approval/Acceptance/Review 决策请求的投影）、产物与交付（Artifact/Workspace/Git）、成本与背压（BudgetLease/CostLedger/RateCard）、故障恢复（Incident/RecoveryPlan；重启后按事件重建运行态、默认暂停等显式恢复）、重试与超时（RetryPolicy/TimeoutPolicy、错误分层、默认不重试）、数据治理与隐私（分类/留存/删除/Secret；记忆保留衰减、免疫排除 candidate/ephemeral、can_read/can_manage 读管分离）。
> - **回答的问题**："不是跑通一次，而是天天跑——谁负责、怎么排队、坏了怎么办、钱花在哪、数据怎么删干净。"
> - **什么时候读**：涉及身份、调度、成本、恢复、数据删除的实现时按章节查阅。
> - **阶段编号**：本文的实施顺序用局部命名 `Ops-0`…`Ops-5`；全局实施阶段以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 为准。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典。

## 1. 目标

CompanyOS 不仅要能够执行一个 Agent Run，还要能够在较长时间内可靠地运行一家公司或个人工作系统：

```text
身份与授权
  → 触发与排队
  → 计划与执行
  → Artifact 交付
  → 成本与容量控制
  → 失败检测与恢复
  → 人类决策与升级
  → 数据留存、删除与审计
```

已有控制面主要回答“请求是否允许”，本文补充：

- 谁是这个请求的责任主体；
- 什么事件可以创建 Run；
- 人类在哪里处理等待中的决定；
- 产物如何版本化、审查、交付和回滚；
- Agent 成本、并发和 Provider 限额如何管理；
- 崩溃、超时、未知结果和数据损坏如何收敛；
- 记忆、Artifact、日志和 Provider 数据如何受到生命周期治理。

## 2. 总体架构

```text
Principal / Organization
          │
          ▼
Identity + Assignment + Data Governance
          │
          ▼
Trigger / Scheduler / Human Inbox
          │
          ▼
Workflow / Agent Run / Capability Broker
          │
          ├── Artifact / Workspace / Delivery
          ├── Cost / Capacity / Backpressure
          ├── Health / Recovery / Incident
          └── Event / Audit / Receipt
```

本文与其他 CompanyOS 文档的关系：

- `company-os-design.md`：总体产品、PMP、权限和安全原则；
- `company-os-domain-contracts.md`：Objective、Project、Acceptance、Delivery、Outcome；
- `company-os-platform-architecture.md`：Runtime、Memory、Context、MCP、Workflow、Swarm、Provider；
- 本文：身份、运营、可靠性和治理；
- `company-os-quality-ecosystem.md`：评测、学习、插件、模型治理和开发者生态；
- `company-os-security-constitution.md`：不可违反的安全宪法和负向验收。

## 3. Identity、组织成员与授权生命周期

### 3.1 Principal 类型

```text
HumanPrincipal       人类用户或审批人
AgentPrincipal       某个 AgentTemplate/Cell 的运行身份
ServicePrincipal     DaemonHost、Scheduler、Projector 等服务身份
McpPrincipal         某个 MCP Server 的受限身份
ProviderPrincipal    外部模型/服务账户的引用身份
```

所有 Principal 都必须有不可变 ID。客户端提交的 `actor_id`、`role_id`、`department_id` 或 `trust` 字段只是请求声明，不能直接成为 authenticated identity。

### 3.2 组织对象

```text
Organization
  ├── Membership
  ├── RoleAssignment
  ├── ProjectAssignment
  ├── ServiceIdentity
  ├── PolicyProfile
  └── DataBoundary
```

目标合同：

```text
Membership {
  membership_id
  organization_id
  principal_id
  status
  roles[]
  scopes[]
  valid_from
  valid_until?
  authority_epoch
}

RoleAssignment {
  assignment_id
  organization_id
  principal_id
  role_id
  department_id
  project_scope[]
  capability_scopes[]
  approver_for[]
  status
  authority_epoch
}
```

`ProjectAssignment`、`ServiceIdentity`、`PolicyProfile` 和 `DataBoundary` 的最小定义如下（均标注 deferred，当前无实现；状态以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准）：

- `ProjectAssignment`：把 `RoleAssignment` 限定到单个 Project 的绑定（`project_id`、`role_assignment_id`、`scope[]`、`valid_until?`）；只收窄 RoleAssignment 的授权，不扩大；
- `ServiceIdentity`：`ServicePrincipal` 的凭据与权限载体（`service_identity_id`、`principal_id`、`capability_scopes[]`、`credential_ref`、`rotation_policy`）；不得承载需要人类确认的价值判断；
- `PolicyProfile`：可被 Membership/RoleAssignment/Project 引用的命名策略集合（`risk_ceiling`、`sandbox_mode`、`approval_policy`、`data_classification_ceiling`）；只能收窄既有策略，不能放宽；
- `DataBoundary`：数据驻留与处理边界（`organization_id`、`allowed_regions[]`、`allowed_providers[]`、`cross_project_sharing`、`retention_override?`）；默认拒绝跨边界，例外必须显式授予。

跨项目共享的最小合同：

```text
SharingGrant {
  sharing_grant_id
  grantor_principal_id     # 必须对 source project 持有共享授权
  source_project_id
  target_project_id
  scope[]                  # Memory/Artifact 引用或数据分类，不允许整库共享
  purpose
  allowed_operations[]
  expires_at
  status
}
```

`SharingGrant` 默认拒绝、只能收窄；没有有效 `SharingGrant` 的跨项目查询 fail-closed。它只解决项目间可见性，具体处理仍需 §9.2 的 `ProcessingGrant`。

### 3.3 身份状态

```text
Principal:
  Proposed → Active → Suspended → Revoked → Archived

Membership:
  Invited → Accepted → Active → Suspended / Expired → Revoked

RoleAssignment:
  Requested → Approved → Active → Reduced / Suspended → Revoked
```

规则：

1. 权限撤销必须增加 `authority_epoch` 或等价 fencing version；旧 Run、Grant、Approval 和 Scheduler trigger 不能继续使用过期 assignment；
2. 新的 Agent Cell 必须从父 Cell、Template、Project 和 Principal 的权限交集中派生；
3. ServicePrincipal 不能代表 HumanPrincipal 直接完成需要人类确认的价值判断；
4. Approval authority 必须明确是 human、policy 还是 delegated approver；
5. 一个 Session 的 owner、workspace、project 和 organization 不能由普通模型文本改变；
6. 跨项目查询必须有显式 `SharingGrant`，不能因为同一用户而自动 union 全部 Memory。

### 3.4 本地优先身份模型

个人本地版可以从以下最小模型开始：

```text
LocalInstance
  → LocalHumanPrincipal
  → ProjectTrust
  → RoleAssignment
  → SessionOwnership
```

本地 bearer token、受保护 Unix socket 或 OS credential 只能解决入口认证的一部分；仍需要服务端从 stored assignment 派生 role、department、project scope 和 authority epoch。

团队/企业版才增加：

```text
Tenant
SSO / OIDC
SCIM
OrganizationMembership
ServiceAccount
AuditPrincipal
KeyRotation
```

### 3.5 Reference 映射

| 参考项目/材料 | 可吸收设计 | Kiana 不直接复制 |
|---|---|---|
| Codex | Thread/Turn identity、rollout reconstruction 和 interrupt boundary | 不把 thread id 当作完整授权边界 |
| OpenCode | session run-state、active run 和 stale response 防护 | 不依赖进程内 Map 保存 ownership |
| Cline | Local Runtime Host 统一持有 runtime 与客户端连接 | 不让 UI 自己生成权限 |
| Crush | RunID correlation、queued/active/terminal 状态 | 不把取消请求本身当作效果已停止 |
| Agent Framework | Approval 绑定原始 FunctionCall identity | 不使用未绑定调用的全局 approval |
| Kiana 当前设计 | RoleSpec、ProjectTrust、Grant 和 Approval | 必须继续补 durable principal 与 authority epoch |

## 4. Trigger、Scheduler 与自动化入口

### 4.1 Trigger 类型

```text
ManualTrigger       用户或 CLI 明确启动
ScheduleTrigger     本地时间表或周期
FileTrigger         文件/目录变化
GitTrigger          commit、branch 或 tag 变化
EventTrigger        Runtime/Project/Incident 事件
WorkflowTrigger     上游 Workflow 完成
ApprovalTrigger     人类批准后继续
ExternalTrigger     受认证的外部 webhook；未来能力
```

### 4.2 TriggerDefinition

```text
TriggerDefinition {
  trigger_id
  owner_principal_id
  organization_id
  project_id?
  source
  filter
  target_kind
  workflow_ref?
  run_template_ref?
  definition_version
  enabled
  timezone?
  schedule?
  concurrency_policy
  deduplication_window
  idempotency_policy
  missed_run_policy
  budget_policy
  approval_policy
  data_scope[]
  created_at
  updated_at
}
```

Trigger 只能提交一个 Workflow 或 Run 请求（由 `target_kind` 决定），不能直接执行 Capability。`target_kind = workflow` 时 `workflow_ref` 必填；`target_kind = run` 时 `run_template_ref` 必填，只引用已登记、版本固定的 `AgentTemplate`，不得内联任意 prompt 或权限。裸 Run 同样必须携带 owner、project scope、`budget_policy`、`approval_policy` 和 expiry，并经过 ControlPlane。

### 4.3 Trigger 状态

```text
Draft → Validating → Enabled
Enabled → Paused / Firing / Disabled / Revoked
Firing → Accepted / Deduplicated / Rejected
Accepted → Queued → Started / Expired / Cancelled
```

每次 firing 必须有：

```text
trigger_firing_id
trigger_id
observed_event_id?
scheduled_at
observed_at
idempotency_key
workflow_instance_id?
run_id?
status
```

### 4.4 调度规则

- 同一个 trigger 的 firing 必须使用稳定 idempotency key；
- 投递语义必须分别定义，不能把本地重试宣称为 exactly-once effect：
  - `at_most_once`：同一 idempotency key 最多接受一次；允许丢失，不允许重复触发。Scheduler 重启后不得重放可能已执行的 firing；
  - `at_least_once`：同一 idempotency key 至少投递一次；允许重复投递，接收侧必须按键去重，保证副作用至多发生一次；
  - `exactly_once`：只承诺命令在 durable store 中被恰好接受一次（按键去重），不承诺外部副作用 exactly-once；外部副作用必须由 ProviderReceipt 或 Reconciliation 确认；
- 并发策略至少支持 `reject`、`queue`、`replace`、`coalesce`；
- Scheduler 重启后必须重新读取 durable definitions；
- 错过计划按 `skip`、`fire_once` 或 `catch_up` 处理；
- 自动触发不提高能力风险等级，也不跳过 Approval；
- 计划暂停、项目关闭、Principal 撤销和 Budget 超限必须阻止新 firing；
- Scheduler 只能创建有 owner、scope、budget 和 expiry 的 Workflow instance 或 Run。

### 4.5 Reference 映射

| 参考项目/材料 | 可吸收设计 | Kiana 约束 |
|---|---|---|
| CrewAI | 事件驱动 Flow、checkpoint 和恢复 | checkpoint 错误不能被吞掉 |
| Archon | DAG、确定性节点、人工 Gate | YAML 只是定义格式，不是权限 |
| ChatDev 2 | Graph node、human/tool 节点 | 不用共享 ChatChain 保存状态 |
| Agno / OpenAI Agents | 可暂停 Run 和 human requirement | 必须持久化 requirement 和 owner |
| Claude Managed Agents 设计 | deployment firing、session/run 分离 | 当前 Kiana 不宣称托管部署能力 |

## 5. Human Inbox、Approval 和升级

### 5.1 人类任务对象

```text
HumanTask {
  human_task_id
  type: approval | review | acceptance | escalation | reconciliation
  owner_principal_id
  project_id?
  workflow_instance_id?
  run_id?
  invocation_id?
  title
  reason
  risk
  payload_digest?
  evidence_refs[]
  due_at?
  expires_at?
  status
  decision_ref?
}
```

`HumanTask` 是 Human Inbox 对控制面决策请求的投影视图：Approval（[`company-os-design.md`](company-os-design.md) §9.4/§10.1）、Acceptance（[`company-os-domain-contracts.md`](company-os-domain-contracts.md) §4.5）、Review 以及 Escalation/Reconciliation 都投影为 HumanTask。权威决策状态在控制面对象上；HumanTask 的 `status` 和 `decision_ref` 只是投影，不能作为授权依据，也不能反向改写 Approval/Acceptance 的状态。

### 5.2 HumanTask 状态

```text
Created → Assigned → Visible → Acknowledged
Visible → Snoozed / Escalated / Expired / Cancelled
Acknowledged → Decided / Rejected / Delegated
Decided → Applied / Failed / ReconciliationRequired
ReconciliationRequired → Incident / Reconciling
Reconciling → Applied / Failed
```

Approval ↔ HumanTask 状态映射（控制面 → 投影）：

| 控制面 Approval 状态（design §9.4） | HumanTask 投影状态 | 说明 |
|---|---|---|
| Staged | Created / Assigned | 未 Assigned 不进入决策人可见范围 |
| Active | Visible / Acknowledged / Snoozed / Escalated | Snoozed/Escalated 只是 Inbox 侧动作，Approval 仍为 Active |
| Approved | Decided | `decision_ref` 指向 Approval，等待消费 |
| Consumed | Applied | 决策已应用到目标 invocation |
| Denied | Rejected | — |
| Expired | Expired | — |
| Cancelled | Cancelled | — |

HumanTask 侧的 `Acknowledged → Delegated` 只重新分配 Inbox 可见性和处理人，不改变 Approval 的 authority scope；真正委托审批权限必须留下 delegation provenance 并仍受 §5.3 约束。`ReconciliationRequired` 不能停留在 HumanTask 内部：必须升级为 `Incident`（需要正式事故记录）或进入 `Reconciling`（可由幂等对账收敛）；收敛结果只能以新事件/新事实追加，不能原地把 `Failed` 改判为 `Applied`。

人类界面至少要显示：

- 触发它的 Objective、Project、Workflow 和 Run；
- 请求者、责任人和审批 authority；
- 最终 payload 和 digest；
- 风险等级和预算影响；
- 证据、来源和数据范围；
- 如果拒绝或过期，后续 Workflow 如何处理；
- 是否存在副作用不确定性。

### 5.3 升级规则

```text
未处理 Approval
  → reminder
  → escalation target
  → timeout policy
  → expire / cancel / human intervention
```

沉默不是批准。Delegated approver 只能在原始 authority scope 内决定，并且必须留下 delegation provenance。

### 5.4 Reference 映射

| 参考项目 | 可吸收设计 |
|---|---|
| Agno | RunRequirement 持久化、pause/continue |
| Agent Framework | approval lifecycle 和原始 call identity |
| Cline | approval callback 与 runtime host 分离 |
| Letta Code | 过期 approval、cursor 和 resume |
| Crush | permission wait、run correlation 和 terminal event |

## 6. Artifact、Workspace、Git 与交付

### 6.1 Artifact 生命周期

```text
Artifact {
  artifact_id
  artifact_type
  project_id?
  run_id?
  source_event_cursor
  content_hash
  parent_artifact_id?
  version
  media_type
  sensitivity
  owner
  retention_policy
  status
}
```

```text
Draft → Versioned → ReviewCandidate → Accepted → Delivered
Draft / Versioned → Rejected / Superseded / Expired / Deleted
Delivered → Confirmed / DeliveryUnknown
```

Artifact 不等于 Event：

- Event 是不可变运行事实；
- Artifact 是可读取、可版本化的产物；
- Receipt 是从事实、Artifact 和验证结果生成的报告；
- Transcript 是可丢弃的展示视图。

### 6.2 Workspace 与 Git

```text
WorkspaceSnapshot
  → Worktree / Branch
  → PatchSet
  → Verification
  → ReviewCandidate
  → MergeCandidate
  → AcceptedArtifact
```

必须记录：

```text
workspace_root
snapshot_hash
base_revision
changed_paths
path_lock_refs[]
author_cell_id
reviewer_cell_id
verification_refs[]
merge_decision
```

规则：

- Builder 的写集必须被 WorkPacket 和 PathLock 限制；
- Review 必须看到明确的 snapshot/base revision；
- Merge 不能覆盖原始 Artifact 或 Evidence；
- 冲突产生新的 MergeConflict/WorkPacket，而不是静默改写；
- 发布或导出必须有目标、版本、接收方和 DeliveryReceipt；
- rollback 产生新的反向动作和事件，不删除历史。

### 6.3 Reference 映射

| 参考项目 | 可吸收设计 |
|---|---|
| Roo Code | 写操作前 checkpoint、model history 与 UI timeline 分离 |
| Pi | append-only history tree、branch/fork 和恢复 |
| Archon | worktree 隔离、实现与审查节点分离 |
| gpt-pilot | task/command 状态链 |
| Aider | diff、repo map、增量上下文 |
| OpenHands | Artifact、事件桥和客户端投影 |

## 7. Cost、Capacity 与 Backpressure

### 7.1 预算层次

```text
FinancialBudget              组织/合同层：支付、收入、合同承诺
  └── ProjectBudget          项目层：项目成本、容量和时间基线
        └── RuntimeBudget    Run 级：token、工具、墙钟和存储限额
              └── BudgetLease     Cell 级派生租约（canonical 名）
ProviderBudget               operations 扩展层：Provider 速率、并发和费用限制
```

- `RuntimeBudget` 是 Run 级预算，`BudgetLease` 是它的 Cell 级派生租约：子 Cell 只能在父级有效租约和项目授权的交集内获得更窄的 `BudgetLease`（[`company-os-domain-contracts.md`](company-os-domain-contracts.md) §4）；
- `BudgetLease` 是 canonical 名（[`company-os-spec-index.md`](company-os-spec-index.md) §4.2）；本文档早期草稿中的 `CellBudget` 与之同义，不再使用；
- `ProviderBudget` 是 operations 扩展层，横切上述预算而不是 `BudgetLease` 的子级；canonical owner 见 [`company-os-spec-index.md`](company-os-spec-index.md) §4.2。

不同预算不能互相替代：

```text
BudgetLease consumed
≠ RuntimeBudget achieved
≠ ProjectBudget achieved
≠ FinancialBudget authorized
```

### 7.2 UsageRecord 与 CostLedger

```text
UsageRecord {
  usage_id
  organization_id
  project_id?
  workflow_instance_id?
  cell_id?
  run_id
  provider
  model
  input_tokens
  output_tokens
  cache_read_tokens
  cache_write_tokens
  tool_calls
  wall_time_ms
  storage_bytes
  external_effect_count
  estimated_cost
  measured_cost?
  created_at
}
```

`estimated_cost` 与 `measured_cost` 的区分标准：

- `estimated_cost`：Provider 回执缺失时，由 `RateCard` 单价 × 已观测用量得出的估算；只用于预警、背压和偏差分析，不得用于 FinancialBudget 结算；
- `measured_cost`：Provider 回执、账单或外部对账提供的实测值，必须携带 `provider_receipt_ref`；只有 measured 能用于结算；
- 同一 `UsageRecord` 两者并存时以 `measured_cost` 为准，同时保留估算值用于偏差分析。

```text
RateCard {
  rate_card_id
  provider
  model?
  currency
  unit_prices          # input/output/cache/tool/storage 等单价
  effective_from
  effective_to?
  version
}
```

`UsageRecord.estimated_cost` 必须引用一个 `RateCard.version`；价格变化只能新增 RateCard 版本，不能改写历史。`RateCard` 的 canonical owner 见 [`company-os-spec-index.md`](company-os-spec-index.md) §4.2。

`CostLedger` 只能追加或通过明确的 correction event 修正，不能覆盖历史消费：

```text
CostCorrection {
  correction_id
  organization_id
  project_id?
  target_entry_ref      # 被修正的 UsageRecord/CostLedger entry；原记录保留
  reason
  delta_estimated_cost?
  delta_measured_cost?
  provider_receipt_ref?
  evidence_refs[]
  requested_by
  approval_ref?
  created_at
}
```

Correction 必须通过 ControlPlane 提交为 versioned command，并由与被修正记录无利益冲突的 human 或显式 delegated approver 批准，产生事件和 Receipt；不能原地覆盖，也不能由模型文本直接修正。修正后的账本视图 = 原始记录 + correction events。

### 7.3 容量与背压

必须限制：

- 每个 Session active Run 数；
- 每个 Project active Cell 数；
- 全局 Provider 并发；
- MCP server 子进程数；
- shell 子进程和 CPU/内存；
- EventLog 写入队列；
- Memory index backlog；
- Workflow trigger firing rate；
- Artifact 和日志磁盘容量。

背压策略：

```text
Accept
→ Queue
→ Delay
→ Coalesce
→ Reject with retry_after
→ Escalate
```

背压不能通过丢弃 Approval、Event、Receipt 或 terminal event 来缓解。

### 7.4 成本指标

```text
cost_per_accepted_delivery   # 唯一 canonical 成本指标名
cache_savings
memory_retrieval_cost
workflow_retry_cost
swarm_duplicate_work_cost
budget_burn_rate
queue_wait_time
```

`cost_per_accepted_delivery = total_provider_cost / accepted_delivery_count`（与 [`company-os-platform-architecture.md`](company-os-platform-architecture.md) §6.3 一致）。单次 Run 的原始成本只作为 `UsageRecord` 明细，不再命名为 `cost_per_run` 指标。必须按 `model/provider/prompt_version/workflow/project` 分桶。平均 token 消耗不是质量指标，核心指标仍是 accepted delivery 和 outcome。

## 8. Reliability、Recovery 与 Incident Operations

### 8.1 运行健康对象

```text
HealthStatus {
  component_id
  component_type
  observed_at
  state: healthy | degraded | unavailable | quarantined
  last_success_at?
  failure_count
  latency
  version
  details_redacted
}
```

适用组件：

```text
DaemonHost
ControlPlane
EventStore
StateProjector
MemoryIndex
ProviderGateway
McpServer
WorkflowScheduler
ArtifactStore
```

### 8.2 失败分类

```text
RetryableTransient
PermanentInputError
PolicyDenied
ApprovalExpired
CapacityRejected
Cancelled
ResultUnknown
PersistenceFailure
SecurityViolation
DependencyUnavailable
```

错误分类必须影响：

- 是否可以自动 retry；
- 是否需要新 Approval；
- 是否必须创建 Incident；
- 是否要进入 Reconciliation 而不是普通 retry；
- CLI/HTTP/Receipt 的最终状态。

错误分类是 §8.5 重试 / 超时判定的输入，不得被降级（例如把 `PolicyDenied`、`SecurityViolation` 当成瞬时错误重试）；分类本身不构成授权。

### 8.3 RecoveryPlan

Incident 的字段与状态机以 [`company-os-domain-contracts.md`](company-os-domain-contracts.md) §4.7 为准；RecoveryPlan 不复制 Incident 状态，只通过 `incident_id` 引用它。

```text
RecoveryPlan {
  recovery_id
  incident_id                 # 引用 domain-contracts §4.7 Incident
  target_aggregate
  observed_state
  last_durable_cursor
  safe_actions[]              # RecoveryAction
  forbidden_actions[]         # ForbiddenAction
  requires_human
  compensation_ref?
  reconciliation_ref?
  status
}
```

`safe_actions[]` 的元素类型是 `RecoveryAction`，`forbidden_actions[]` 的元素类型是 `ForbiddenAction`：

```text
RecoveryAction {
  action_kind
  target_ref
  idempotency_key
  requires_approval
  expected_effect
  verification_ref?
}

ForbiddenAction {
  action_kind
  target_ref
  reason
}
```

```text
Proposed → Approved → Executing → Verified / Failed
Proposed / Approved → Abandoned
```

授权规则：

- 创建（`Proposed`）：由 Incident owner 或 on-call `ServicePrincipal` 提交，必须绑定 `incident_id` 与 `observed_state`；模型文本不能直接创建 RecoveryPlan；
- 批准（`Approved`）：`requires_human = true` 或 `safe_actions` 中存在 `requires_approval` 的动作时，必须由 ControlPlane 记录的 human/policy approver 批准；RecoveryPlan 不能自批准；
- 执行（`Executing`）：每个有副作用的 action 需要新的授权和 idempotency key（见 §8.4）；同一 `target_aggregate` 同时只允许一个处于 `Executing` 的 plan；`forbidden_actions` 只增不减，执行前必须逐条校验；
- 收敛：`Verified` 必须引用验证证据；`Failed` 必须产生新事件，并升级为新的或关联的 Incident。

重启后的运行态重建（新增规范）：

- 重启后不假设内存态可用：按 EventLog 折叠重建 Run / Invocation 投影与待审批集合（投影形状见 [`company-os-platform-architecture.md`](company-os-platform-architecture.md) §8.4），`RecoveryPlan.observed_state` 与 `last_durable_cursor` 必须与该投影一致；
- 重建出的 pending 项只回答"有哪些单子、在等什么"：必须重新经过 policy / gate / approval，不得直接交给 broker，也不得把重建本身当作授权；
- 默认暂停（fail-closed）：重建完成后系统停留在暂停态，不自动续跑任何 Run；只有用户显式"恢复"才继续，恢复动作本身是一条可审计事件；
- 重建与恢复只 append 事件，不改写历史事件或既有终态；无法从账本确定的状态一律 `result_unknown`。

统一恢复流程：

```text
Detect
  → Classify
  → Fence
  → Persist incident
  → Retry / Reconcile / Compensate
  → Verify
  → Resume or Close
```

### 8.4 关键故障规则

- EventLog 追加失败时，不能发布“已完成”；
- Projector 失败时，不能删除原始事件，必须允许重放；
- Provider timeout 不等于 effect 未发生；
- MCP process crash 后不能自动重复不幂等调用；
- Cancel 返回后若不能确认 handler 停止，必须停留在 `cancel_requested`（UI 投影名 `cancelling`，不是状态或终态）或进入 `result_unknown`；
- 磁盘满、半写 JSONL、损坏 Artifact 和失联子进程必须可观测；
- Recovery action 本身如果有副作用，也必须经过新的授权和 idempotency key；
- Incident 关闭必须引用验证证据，不依赖模型说“已经恢复”。

### 8.5 重试、超时与尝试记录

`RetryPolicy` / `TimeoutPolicy` 是可持久化的一等策略：随 `CapabilityRequest` / Invocation / `WorkflowDefinition` 一起落盘并版本固定，不得写死为常量；字段形状与 [`company-os-platform-architecture.md`](company-os-platform-architecture.md) §4.4 统一。运营侧规则：

- **fail-closed 默认不重试**（`maximum_attempts = 1`）：没有显式策略声明时一律单次尝试，非幂等操作禁止重试；
- **错误分层**（由 §8.2 的分类派生，分类不得降级）：
  1. 传输 / 模型瞬时错误：仅当尚无 capability 被真正执行时，才做有界退避重试；
  2. 模型可纠正的工具错误：作为工具结果回灌模型，不判 Run 失败，也不消耗重试额度；
  3. `PolicyDenied`、`SecurityViolation`、指纹不符、调用账本显示已执行：绝不重试；
- **每次 attempt 写独立事件**，`attempt` 单调递增，旧 attempt 事件不可改写；Invocation 终态取最近一次 attempt 的终态，只要存在副作用未确认的 attempt 必须为 `result_unknown`；
- 超时（`start_to_close` / `schedule_to_close` 到达）或结果未知一律收敛为 `result_unknown`，绝不自动重试；
- 重试前必须查调用账本（[`company-os-platform-architecture.md`](company-os-platform-architecture.md) §4.1）：已执行直接返回缓存结果或 fail-closed，指纹不一致立即拒绝；
- 重试不提高能力风险等级、不跳过 Approval，也不得因换沙箱执行而绕过任何 deny 条目。

### 8.6 Reference 映射

| 参考项目 | 可吸收设计 |
|---|---|
| Goose | effect-before-event、状态机重载、步骤级恢复 |
| Crush | queued/active cancel、terminal event 和 dispatch race 测试 |
| DeepSeek Harness | 子进程组、工具边界和确定性 failure fixture |
| OpenCode | projector、hydration、retry/compaction 关系 |
| Cline | abort 队列、host/runtime 生命周期 |
| CrewAI | checkpoint event，但不能吞掉 checkpoint failure |

## 9. Data Governance 与 Privacy

### 9.1 数据分类

```text
Public
Internal
ProjectConfidential
UserPrivate
CredentialSecret
PersonalData
SensitivePersonalData
ExternalProviderData
OperationalSecurityData
```

每个 Memory、Artifact、Event payload、Tool result 和日志字段都必须有分类或默认分类。

### 9.2 Purpose 与 ProcessingGrant

```text
ProcessingGrant {
  grant_id
  principal_id
  data_refs[]
  purpose
  allowed_operations[]
  allowed_destinations[]
  expires_at?
  retention_policy
  consent_ref?
  status
}
```

允许 Agent 读取某个 Project Memory，不代表允许：

- 把它送给任意 Provider；
- 写入 Company-wide Memory；
- 复制到别的 Project；
- 放进长期 prompt cache；
- 作为外部邮件或订单内容。

### 9.3 数据生命周期

```text
Collected
  → Classified
  → Scoped
  → Used
  → Shared / Exported
  → Archived
  → Expired / Deleted
```

删除或修正必须传播到：

```text
Primary record
→ Event projection
→ Artifact index
→ Vector index
→ Graph index
→ Compaction summary
→ Provider-bound cache policy
→ Search result cache
```

安全事件、法律留存和审计证据可以有不同 retention，但必须由显式 policy 决定。

**记忆保留、衰减与授权谓词（新增规范）**：

- 记忆的保留期到期与相关性衰减由确定性 sweep 决定：relevance 分数与衰减曲线必须是可注入时钟 / 输入摘要下的确定性函数，同样的输入重放得到同样的过期集合；sweep 只 append `MemoryExpired` / `MemoryStale` 事件并更新投影，不得原地改写或删除原始记录（清理责任见 [`company-os-platform-architecture.md`](company-os-platform-architecture.md) §5.3）；
- 免疫规则（豁免 sweep 的记录）必须显式排除 `candidate` / `ephemeral`：这两种准入状态永不免疫，必须参与过期与衰减；
- 分层默认可见性：`instance/<session_id>/scratch` 层默认可见（临时草稿），持久层默认 `candidate` 不可检索，只有显式批准后才进入默认检索；
- 检索与写入统一由两个独立谓词裁决：`can_read` 与 `can_manage`（能读 ≠ 能管），各自独立判定、默认拒绝、fail-closed；任何检索通道不得绕过 `can_read`，任何写入 / 晋级 / 失效路径不得绕过 `can_manage`；排序分数只进排序，不参与授权。

### 9.4 Secret 规则

Secret 原值不得进入：

```text
Prompt
Transcript
RuntimeEvent payload
Receipt
stdout/stderr
argv
Memory
Cache artifact
Error string
```

只能通过受控 Broker 的短期 invocation handle 解析。日志、指标和错误只记录 redacted metadata。

### 9.5 数据治理 Reference

参考 Agent 项目更多提供的是 runtime 边界，不是完整隐私系统；Kiana 应吸收：

- Cline/Goose 的 host/tool boundary；
- OpenCode/Roo 的工具结果和历史分离；
- Letta/Continue 的分层 session/memory；
- 12-factor 的最小上下文原则；
- 本仓安全宪法关于 Secret、Memory ACL 和 provenance 的约束。

不能把“有 Memory ACL”误称为完成了 retention、删除、consent 和 Provider processing governance。

## 10. Integration 与 Connector 边界

未来外部 Connector 统一采用：

```text
ConnectorDefinition
  → AccountBinding
  → CapabilityDescriptor
  → DataProcessingPolicy
  → HealthCheck
  → Approval
  → Invocation
  → ProviderReceipt
  → Reconciliation
```

Connector 不得直接写 Project 或 Memory 状态；只能通过 versioned command/event 和 ControlPlane 改变状态。

Connector 需要明确：

- 外部账户身份；
- 读取和写入 scope；
- webhook 签名/重放防护；
- rate limit；
- provider receipt；
- cancellation/refund/reconciliation；
- 数据留存和跨境处理；
- version 和 schema drift；
- 断线恢复；
- 删除和撤销。

当前不应先做大量 Connector。优先做本地、只读、可回放的 adapter fixture。

## 11. 运营完成定义

达到 `local_behavior` 前至少需要：

- 一个本地 Principal 可创建、暂停、撤销 RoleAssignment，且跨项目读取受 `SharingGrant` 控制；
- Session/Run/Workflow 不依赖全局 active 状态；
- Schedule、Manual、Event trigger 都是可重放、可去重的命令；
- Approval、Review、Acceptance、Reconciliation 都有 Human Inbox 投影；
- Artifact、Workspace、Patch 和 Delivery 可从 Event/Receipt 回溯；
- CostLedger 能区分 BudgetLease、RuntimeBudget、ProjectBudget 和 FinancialBudget，且 correction 全部可追溯到 ControlPlane 批准的 `CostCorrection`；
- crash、timeout、cancel、disk full、MCP failure 和 `result_unknown` 都有 Incident/RecoveryPlan，且 RecoveryPlan 的 `Proposed → Approved → Executing → Verified / Failed / Abandoned` 全程可审计；
- 删除和 retention policy 不会留下未治理的 Memory/Index/Cache 副本；
- 外部 Connector 不会绕过 ControlPlane、Approval、Idempotency 和 Receipt。

## 12. 实施顺序

全局实施阶段以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 为准（Human Inbox 在全局是 P2，Scheduler 在全局是 P4）；下表 `Ops-*` 只是本文覆盖范围的局部顺序，不是全局 P 编号。

```text
Ops-0 durable principal + ownership + event/recovery
  ↓
Ops-1 human inbox + cost/quota + artifact snapshot
  ↓
Ops-2 local trigger/scheduler + incident/reconciliation
  ↓
Ops-3 data classification + retention/deletion + provider processing policy
  ↓
Ops-4 connector SDK + webhook/event ingress
  ↓
Ops-5 team/tenant/remote operations
```

在 Ops-0–Ops-2 之前不做：

- 自动支付；
- 无人值守外部发送；
- 远程执行；
- 多租户共享 Memory；
- 大规模 Marketplace Connector；
- 无人值守的物理设备操作。

## 13. 当前诚实描述

> **Kiana 已有本地 ControlPlane、ProjectTrust、Approval、EventLog、Artifact 和部分运行安全能力；持久化 Principal、完整 Scheduler/Human Inbox、成本账本、跨进程恢复、Incident/Reconciliation、数据删除传播和外部 Connector 治理仍是目标能力。**
>
> 上述"已有"能力的证据以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) §2 能力表和既有证据块为准，例如 `S0 concurrency and fail-closed correction evidence (2026-09-01)`、`P1-02 structured approval digest evidence (2026-08-29)`、`P1-04 governance artifact symlink and atomic-write evidence (2026-09-07)` 与当前 Gate 0 证据块；本段不新增证据，也不提升证明等级。
