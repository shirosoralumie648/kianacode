# Kiana Company OS 总体设计与实施规范

> 文档状态：Draft / Implementation Baseline
> 适用范围：Kiana 本地控制面、个人 Company OS、未来团队与生活能力域
> 领域合同补充：[`company-os-domain-contracts.md`](company-os-domain-contracts.md)
> 平台能力补充：[`company-os-platform-architecture.md`](company-os-platform-architecture.md)
> 证据上限：当前 checkout 只能宣称 `local_behavior`

> **本文速览（导读，非规范）**
>
> - **讲什么**：CompanyOS 的总蓝图——产品定位与北极星、治理/劳动/能力三个层次、五个 PMP 部门、核心对象词典（WorkPacket、Cell、Grant、Receipt…）、细胞分裂的防失控规则、R0–R5 风险分级、关键状态机（Run、Cell、WorkPacket、CapabilityExecution、Approval）、安全宪法总纲（与 SEC-01–SEC-12 的映射）和以规范索引 §7 为准的分阶段路线图。
> - **回答的问题**："Kiana 想成为什么样的系统、按什么原则运转、先做什么后做什么。"
> - **什么时候读**：想理解整个系统的设计逻辑时从这份开始；其余各份规范都是它某一部分的展开。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典；当前实际能力以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准。

## 0. 设计结论

Kiana 不是一个拥有更多工具的聊天机器人，而是一个由人类 Sponsor 授权、由控制面治理、由专业 Agent 部门完成交付的 Company OS。

```text
Sponsor 目标
  → 成功标准与边界
  → PMP 治理选择
  → WorkPacket
  → Agent Cell 执行
  → Capability 授权
  → 独立验证
  → 人类决策
  → Receipt / 复盘 / 记忆
```

Kiana 的产品北极星是：

> **每个活跃项目周期内，人类接受的、验收通过且完整可追溯的交付闭环数。**

不以 Agent 数量、工具数量、Token 消耗、聊天轮数或会话时长作为核心成熟度指标。

---

## 1. 证据账本：现状、目标与禁止表述

### 1.1 当前可证明：`local_behavior`

当前只能基于固定源码快照、受信任本地仓库以及 cassette/fake-script 证明：

- `kiana-entrypoints → DaemonHost → ControlPlane → KianaHarness` 主路径；
- CLI、Workbench 和 loopback Web 使用同一控制面组合根；
- 模型工具调用转换为 capability request，而不是直接执行；
- trust、sandbox、role、path、memory grant 和 policy/gate 已进入现有领域/控制面；
- 本地 shell、apply_patch 等能力存在受控路径；
- WorkPacket、Symposium、Review、EventLog 和 Receipt 存在部分实现；
- Desktop 是承载 Web/daemon 的本地壳；
- Web 当前不声称 token streaming。

### 1.2 当前明确不能宣称

以下属于目标或未来能力，不得写成现状：

- live provider 已完成；
- token streaming 已完成；
- 跨进程或跨机器 resume 已完成；
- 生产级安装、签名 `.deb` 或成熟自动升级；
- 企业级租户、RBAC、云协作和合规审计；
- 支付、打车、外卖、订票、订房和 IoT 已接入；
- HTTP MCP、远程执行、computer-use 已完成；
- Receipt 已证明现实世界结果正确；
- 当前 dirty checkout 全部测试通过。

### 1.3 现有文档权威层级

权威层级以 [`company-os-spec-index.md`](company-os-spec-index.md) §2 为准：

```text
源码 + 精确测试/Smoke 回执 + CURRENT_STATUS.md
  → README.md / USER.md 当前用户面
  → 安全宪法 + 已批准 ADR
  → CompanyOS 领域/平台规范
  → 实施大纲 + PHASES.md + PROCESS.md + DESIGN.md
  → reference-agent-audit/ 和 reference/ 审计材料
```

| 文档 | 权威职责 |
|---|---|
| `CURRENT_STATUS.md` | 当前状态、证明等级、Gate 结果和已知限制；当前事实的唯一汇总入口 |
| `README.md` / `USER.md` | 当前用户面与可运行证据 |
| `company-os-security-constitution.md` | 安全宪法 SEC-01–SEC-12、风险等级、负向验收和发布门；安全条款 canonical |
| 已批准 ADR | 架构决策记录，与安全宪法同级约束 |
| `company-os-spec-index.md` | CompanyOS 规范索引、文档地图和 canonical owner 注册表 |
| `company-os-domain-contracts.md` | Objective、Project、Milestone、Acceptance、Delivery、Outcome 等业务事实 |
| `company-os-platform-architecture.md` / `company-os-operations-governance.md` / `company-os-quality-ecosystem.md` / `company-os-ui-ux.md` / `company-os-reference-matrix.md` / `schemas/README.md` | 各领域规范目标、运营治理、质量生态、UI 投影、参考映射和机器合同 |
| 本文档 | Company OS 规范目标、状态契约、迁移和实施 backlog |
| `company-os-implementation-outline.md` / `PHASES.md` / `PROCESS.md` / `DESIGN.md` | 实施切片、阶段剧本、证据发布流程和版本决策 |
| `COMPANY.md` | Company OS 愿景和组织模型 |
| `CLAUDE.md` / `AGENTS.md` | 当前架构约束、命令和协作规则；通用模板命令不作为 Kiana 事实 |
| `reference-agent-audit/`、`reference/` | 审计材料，仅作参考，不作为 Kiana 证据 |

当文档互相冲突时，必须先建立 Evidence Ledger，再修改产品声明；不得用放宽测试断言的方式消除冲突。

---

## 2. 产品层次

### 2.1 治理层

治理层维护：

```text
Organization → Portfolio → Program → Project
```

- **Organization**：个人 Company 或未来租户根；
- **Portfolio**：战略目标、优先级和资源投入集合；
- **Program**：共享目标、依赖和风险的一组项目；
- **Project**：有范围、成功标准、可选工作区引用（`workspace_ref?`，与 domain-contracts 对齐）和生命周期的交付单元。

### 2.2 劳动层

劳动层维护：

```text
Department → RoleSpec → AgentTemplate → AgentInstance/Cell
```

Agent 是短生命周期、有预算、有权限、有输入输出契约的执行单元，不是拥有全局权限的数字员工。

### 2.3 能力层

能力通过受控适配器提供：

```text
Coding
Office / Work
Search / Recommendation
Commerce / Food
Mobility / Travel
Home / IoT
```

所有有副作用的能力都必须经过：

```text
Agent
  → CapabilityRequest
  → ControlPlane
  → Policy / Gate / Approval
  → CapabilityGrant
  → Broker
  → Adapter
  → Effect
```

---

## 3. PMP 部门模型

五个 PMP 过程组对应五个常设部门，但不是强制线性流水线。

| 部门 | 过程组 | 责任 |
|---|---|---|
| Initiating | 立项部 | 价值、问题、范围、成功标准、Sponsor go/no-go |
| Planning | 规划部 | WBS、依赖、风险、预算、WorkPacket 和验收 |
| Executing | 执行部 | Builder 在授权范围内产出变更和证据 |
| Monitoring & Controlling | 监控部 | 进度、风险、变更、独立 Review 和 Gate |
| Closing | 收尾部 | Receipt、验收、关闭、复盘和记忆写回 |

流程按最小充分原则选择：

| 级别 | 任务 | 流程 |
|---|---|---|
| L0 | 单文件、小型只读或简单可逆动作 | 直接 Builder |
| L1 | 多文件或需要验收的本地变更 | WorkPacket + Builder + Verification |
| L2 | 有依赖、风险、变更或跨部门协作 | Charter + Plan + Execute + Review + Close |
| L3 | 多项目持续运营 | Portfolio + Program + 资源和长期记忆 |
| L4 | 团队、远程、企业 | authenticated identity + tenant/RBAC + remote governance |

PMP 提供治理词汇，不应被实现为每个小修改都必须参加的五次会议。

---

## 4. Sponsor、Boss 与 Kiana

### 4.1 Sponsor/Boss 的职责

Sponsor 是最终的人类决策者，负责：

- 目标和成功标准；
- 优先级和资源；
- 风险承受度；
- 范围、预算和外部效果授权；
- 高风险操作的最终确认；
- 接受、拒绝或要求变更。

“Boss”是产品角色，不是绕过控制面的超级管理员。

### 4.2 Kiana 的职责

Kiana 负责：

- 澄清意图；
- 选择最小充分流程；
- 选择预制 Agent 模板；
- 生成和派发 WorkPacket；
- 监督 Cell、预算、锁和依赖；
- 组织验证和独立 Review；
- 解释阻断和失败；
- 生成 Receipt；
- 将筛选后的经验写回适当记忆层。

Kiana 不替 Sponsor 做未经授权的价值判断、支付、签约或安全关键动作。

---

## 5. 领域对象词典

### 5.1 WorkPacket

WorkPacket 是跨部门唯一正式交接单位，必须能够独立授权、执行、验收和生成 Receipt。

```text
WorkPacket {
  packet_id
  project_id
  milestone_id?
  parent_packet_id?
  from_department
  to_department
  owner_cell_id
  acceptor_id
  goal
  inputs[]
  dependencies[]
  owned_paths[]
  data_scope[]
  acceptance_tests[]
  forbidden_actions[]
  deadline
  budget_lease_id
  status
  schema_version
}
```

`goal` 与代码、`company-os-domain-contracts.md` 和 overview 的字段名保持一致。`milestone_id?` 为新增规范字段（当前 `kiana-domain::WorkPacket` 尚未提供）：用于把工单挂到里程碑，使验收标准可以沿 packet → milestone → project 上溯；不填写时该 packet 只能作为 standalone 工作，不得作为 Project 交付证据。

必须指定 domain WorkPacket 为 canonical 类型。`kiana-tasks` 等旧类型只能作为调度投影或兼容 adapter，不得形成第二个真相。

验收标准分四层派生并冻结：packet 的 `acceptance_tests[]` 必须落在所属 Milestone 的 `acceptance_criteria[]` 内，Milestone 的标准必须落在 Project 的 `success_criteria[]` 内；`Acceptance` 进入 `Requested` 时从已冻结的上层标准派生并冻结为 `Acceptance.criteria_snapshot`，决策阶段只读校验、不得改写，任何修改都必须走 ChangeRequest 并产生新版本，不能覆盖历史标准（详见 `company-os-domain-contracts.md` §4.3–§4.5）。

### 5.2 Request / Session / Run / Turn

```text
request_id       一次客户端意图
session_id       长期会话容器
run_id           一次执行生命周期
turn_id          一次输入到输出的交互轮次
execution_id     一次具体 capability 执行
invocation_id    一次可恢复的能力调用
```

这些 ID 不得混用。重试、continue、取消和审批必须在契约中分别定义。

### 5.3 AgentTemplate

```text
AgentTemplate {
  template_id
  version
  role
  mission_schema
  input_schema
  output_schema
  default_capabilities
  sandbox_profile
  estimated_cost
  max_children
  max_depth
  ttl
  heartbeat_interval
  checkpoint_policy
  merge_strategy
}
```

模板版本固定；Agent 运行时不能修改模板、角色或默认权限。

### 5.4 Agent Cell

```text
CellSpec {
  cell_id
  parent_cell_id?
  root_run_id
  template_ref
  role
  objective
  input_refs[]
  output_contract
  partition_key
  owned_paths[]
  capability_grant_id
  budget_lease_id
  supervision_lease_id
  depth
  spawn_quota
  lifecycle
}
```

每个 Cell 只有一个 owner 和一个责任父节点。

### 5.5 SpawnPlan

```text
SpawnPlan {
  plan_id
  parent_cell_id
  reason_code
  candidate_templates[]
  count
  partition
  input_refs[]
  output_contract
  requested_capabilities[]
  budget_reservation
  deadline
  rollback_policy
  idempotency_key
  expected_utility
}
```

Spawn 必须先 validate/authorize，再原子预留预算、资源锁和 Grant，最后写 `SpawnCommitted` 事件。

### 5.6 Grant、Lease 和 Receipt

- **CapabilityGrant**：能力、动作、资源、路径、TTL、approval_ref 和不可转授约束；
- **BudgetLease**：token、工具调用、墙钟、并发和效果数量硬限额；
- **SupervisionLease**：heartbeat、checkpoint、stall threshold 和 retry limit；
- **MergeReceipt**：只有通过契约、测试、权限和冲突检查的结果才能进入父输出；
- **RetirementRecord**：撤销 Grant、释放锁、保存 checkpoint、产物和剩余预算的最终记录；
- **ClosingReceipt**：项目级最终交付、验证、审查、接受和例外记录。

Grant、Lease、Receipt 都是控制面产生的对象，不能由 Agent 自报或通过普通 Artifact 伪造。

---

## 6. 细胞分裂协议

### 6.1 Kiana 的决策步骤

1. 把目标规范化为 WorkGraph；
2. 标记节点依赖、风险、输入输出和写集；
3. 从 TemplateRegistry 选择最小覆盖模板；
4. 只有存在互斥分区时才并行；
5. 计算预算、深度、数量和 TTL；
6. 生成 SpawnPlan；
7. 由 ControlPlane 做策略和一致性校验；
8. 创建 Cell 并写入事件；
9. 监督、合并和收敛。

### 6.2 防无限分裂

必须同时限制：

- root total cells；
- parent max children；
- max depth；
- 全局并发；
- retry/attempt；
- 墙钟；
- token/tool/effect/storage/CPU 配额；
- spawn rate；
- 最小和最大 TTL。

`delegation_allowed` 默认是 false。子 Cell 只有在显式 SpawnGrant 中获得可降权的 spawn 能力时才可继续分裂。

SpawnGrant 不是独立于 CapabilityGrant 的第二套授权：它是 `CapabilityGrant` 在 `spawn` operation 上的特化视图，必须并入父级 Grant 的 spawn 能力，并同时满足降权约束——候选模板集合、数量、深度、TTL、预算和并发都不得超过父级。它不能新增父级未拥有的能力，也不能替代 §6.1 的 SpawnPlan 校验。

DelegationPacket 是父子 Cell 之间的运行时授权信封，其最小合同为：

```text
DelegationPacket {
  schema
  delegation_id
  parent_cell_id
  child_cell_id
  source_packet_id
  capability_scopes[]
  path_scopes[]
  budget_lease_id
  capability_grant_id
  supervision_lease_id
  expires_at_unix_ms
  delegation_allowed
}
```

边界：`WorkPacket` 是业务交接单位（目标、验收、责任），不携带运行时授权；`SpawnPlan` 是分裂申请（候选模板、数量、分区、预算预留和回滚），决定"要不要创建 Cell"；`DelegationPacket` 只记录"已创建的父 Cell 向子 Cell 授了什么、到什么时候、能不能再委派"。三者不是同一个对象，也不能互相替代（见 `company-os-spec-index.md` §4.2、§6.3）。

### 6.3 防重复劳动

使用：

```text
WorkFingerprint
= canonical objective
+ input versions
+ partition
+ output contract
+ policy snapshot
```

相同指纹的活跃任务应复用、订阅结果或拒绝重复创建，而不是无条件复制。

### 6.4 防权限扩散

子权限必须满足：

```text
child_grant
⊆ parent_effective_grant
∩ template_grant
∩ department_policy
∩ project_policy
∩ packet_scope
∩ approval_scope
```

子 Agent 不能获得：

- 父级未拥有的能力；
- 更宽路径；
- 网络或 secret 权限；
- 新的外部账户；
- 永久 TTL；
- 默认再委派权。

### 6.5 防责任逃逸

- parent、root、packet、owner、acceptor 不可变；
- 接收方 ACK 前，原 owner 仍负责；
- 沉默不能等同于成功；
- 输出必须是 typed result 或 typed failure；
- 失败重试创建新 attempt，不复活旧责任链；
- MergeReceipt 必须包含 reviewer/acceptor 和 provenance；
- Retire 后 Grant、锁和预算必须撤销或释放。

---

## 7. 通信、汇报与问责

### 7.1 消息类型

| 类型 | 是否产生正式效果 |
|---|---|
| Chat | 否，只是讨论 |
| Command | 是，须经 ControlPlane |
| Handoff | 是，必须 ACK |
| Decision | 是，必须有 authority chain |
| StatusReport | 更新进度和阻塞，不转移责任 |
| Evidence | 提交可验证产物 |
| Incident | 触发升级、隔离或人工处理 |

不能把自由 `SendMessage` 或共享 transcript 作为跨部门总线。

### 7.2 StatusReport

```text
StatusReport {
  cell_id
  packet_id
  status
  progress
  blockers[]
  budget_used
  outputs[]
  evidence_refs[]
  next_action
  escalation
}
```

### 7.3 责任链

```text
Sponsor
→ Project
→ WorkPacket
→ Parent Cell
→ Child Cell
→ CapabilityExecution
→ Evidence
→ Reviewer
→ MergeReceipt
→ Acceptance
→ Delivery
→ ClosingReceipt
```

`MergeReceipt` 是执行层回执：证明合并结果通过了契约、测试、权限和冲突检查，可以进入父输出；`Acceptance` 是业务层决策：由独立决策者依据冻结的 `criteria_snapshot` 接受、拒绝或要求返工。执行层合并成功不能自动推导为业务验收通过（见 `company-os-domain-contracts.md` §4.5、§5.2）。

部门、角色、收件人和 `from/to` 字段都是声明，不能代替真实的服务端 authority chain。

---

## 8. 能力域与风险分级

统一采用 R0–R5，并按副作用、敏感性、资金、法律、物理危险和可逆性正交计算，取最高风险。

赋级规则：能力 descriptor 必须声明该能力及每个 operation 的默认风险等级，ControlPlane 在服务端依据实际 operation、目标资源、数据范围和副作用重新校验；同一动作涉及多个维度时取最高级别，就高不就低。模型、网页、provider 返回值和普通消息都不能单方面降级或提级，descriptor 声明本身也不构成授权。

| 等级 | 定义 | 示例 | 授权要求 |
|---|---|---|---|
| R0 | 只读/观察 | 读代码、搜索、查路线、读设备状态 | 最小读取 scope |
| R1 | 可逆本地写入 | 改文件、生成草稿、整理笔记 | 路径/资源授权，快照或 undo |
| R2 | 个人事务/对外草拟 | 邮件草稿、预填表单、行程草稿 | 显示草稿和提交目标 |
| R3 | 外部非金钱副作用 | 发消息、提交工单、发布内容 | 最终 payload 单次确认 |
| R4 | 金钱/预订/法律承诺 | 支付、下单、订票、订房、签约 | 二次确认、预算和幂等 |
| R5 | 物理/高后果动作 | 解锁、驾驶、燃气、高功率、医疗设备 | 默认禁止自治、人在回路 |

> **开放决策（待 ADR）**：六个维度到 R0–R5 的判定标尺尚未定义；当前 `kiana-domain::RiskLevel` 只有 `ReadOnly`、`LocalWrite`、`ExternalSideEffect`、`Critical` 四档，与 R0–R5 的版本化映射表待定。本规范不自行发明该映射（兼容边界见 `company-os-spec-index.md` §6.3）。

### 8.1 Coding

当前优先支持：

- read/search/test；
- shell；
- apply_patch；
- context；
- stdio MCP；
- memory；
- skills；
- hooks。

shell 在 `workspace-write` 下必须被视为高风险能力，限制 cwd、环境、进程树、时长和输出，不能只依靠路径字符串检查。

### 8.2 Office / Work

允许生成和整理草稿；发送邮件、更新外部任务、发布内容必须升级为 R3，并展示最终 payload。敏感联系人、会议、健康和组织数据需要目的绑定与最小 scope。

### 8.3 Search / Recommendation

搜索、比价和推荐不产生购买授权。网页、搜索结果和 provider 返回值都是不可信输入，不能修改角色、Grant 或预算。

### 8.4 Commerce / Food

下单、支付、退款、订阅和地址修改属于 R4。approval 必须绑定最终商户、商品、数量、总价、税费、配送、地址、时间和退改条款。任何漂移都使 approval 失效。

### 8.5 Mobility / Travel

路线、报价、票价和酒店信息可以先做只读查询。打车下单、出票、订房、改签和取消需要明确确认、幂等键、供应商状态核验和未知结果处理。

### 8.6 Home / IoT

设备读取和场景草稿可以是 R0/R1。门锁、车库、摄像、麦克风、燃气、加热、高功率、安防、固件、配网和车辆控制属于 R5，默认禁止自治，必须有独立 safety controller、watchdog、急停和人工接管。

---

## 9. 状态机与一致性

### 9.0 Run

Run 是一次执行生命周期（见 §5.2）。状态集合与 wire 名：

```text
accepted            已接受请求，不代表已授权
running             正在执行
awaiting_approval   等待审批
blocked             被依赖、预算或外部条件阻断
cancel_requested    已请求取消，等待停止确认
completed           成功终态
failed              失败终态
cancelled           取消已确认的终态
denied              拒绝终态
result_unknown      结果未知终态
```

合法转移：

```text
accepted → running / awaiting_approval / blocked / denied
running → awaiting_approval / blocked / completed / failed / cancel_requested / result_unknown
awaiting_approval → running / blocked / denied / failed / cancel_requested / result_unknown
blocked → running / failed / cancel_requested
cancel_requested → cancelled / result_unknown
```

命名统一：

- `completed` 是执行层的成功终态 wire 名；`Succeeded` 是同一成功在 Cell/WorkPacket 聚合上的状态名。二者语义对应，但不是同一个聚合状态，不得互相替代或形成第二个成功状态；
- 取消中间态的 wire 名是 `cancel_requested`；`cancelling` 只是 UI 投影名，不是状态、不是终态。只有停止被确认后才能进入 `cancelled`；无法确认副作用是否停止时只能进入 `result_unknown`，且 `result_unknown` 是唯一未知终态。

终态集合：`completed`、`failed`、`cancelled`、`denied`、`result_unknown`。

非法转移包括：任何终态回到运行态；`cancel_requested → completed`；`awaiting_approval → running` 而未经有效审批决定；`result_unknown` 原地改判为 `completed`/`failed`/`cancelled`（只能经 reconciliation 追加新事实）。

> **开放决策（实现缺口）**：当前 `kiana-domain::ExecutionStatus` 没有 `cancel_requested` 中间态，取消确认与 Unknown 的区分仍在 P1-02/P1-03 的 `partial` 范围内（见 `CURRENT_STATUS.md`）。

### 9.1 Cell

```text
Proposed → Validated → Spawning → Ready → Running
Running → WaitingInput / Blocked / Checkpointing / ReadyToMerge / Stalled / Failed / Quarantined
WaitingInput / Blocked → Running
Checkpointing → Running / ReadyToMerge
ReadyToMerge → Merging → Succeeded
ReadyToMerge → Retiring
Stalled → Retrying → Running / Failed
Running → CancelRequested → Cancelled
Failed → Quarantined
Succeeded / Cancelled / Quarantined → Retiring → Retired
```

所有终局 Cell 都必须经过 `Retiring` 释放 Grant、锁和预算，再进入 `Retired`；在 `Retiring` 完成前不得对外宣告终局，`Retired` 是正常收敛的唯一终点。

### 9.2 WorkPacket

```text
Draft → Approved → Assigned → Running
Assigned → Running（接收方 ACK 后）
Running → Blocked / AwaitingApproval / Succeeded / Failed / Cancelled
Blocked / AwaitingApproval → Running
Succeeded → Reviewed
Reviewed → Closed / Running（rework）
任一非终态 → Cancelled
```

`Assigned` 只表示 Handoff 已发出（§7.1：Handoff 必须 ACK），不表示责任已经转移；接收方 ACK 之前责任仍在原 owner（§6.5），且不得进入 `Running`。`Running` 的前置条件是有效的 HandoffReceipt；沉默、超时、StatusReport 或普通消息都不构成 ACK。

HandoffReceipt 的最小合同（新增规范文本，当前代码尚无对应结构）：

```text
HandoffReceipt {
  receipt_id
  packet_id
  from_owner
  to_owner
  ack_by
  acked_at
  payload_digest
  schema_version
}
```

`Reviewed → Running` 表示 Review 要求返工；返工必须重新进入 `Running` 并保留原 Review 事实，不能改写或删除（与 `company-os-domain-contracts.md` §4.5 的 `Rejected → ReworkRequested → EvidencePending` 对齐）。

> **开放决策（实现缺口）**：当前 `kiana-domain::WorkPacketStatus` 没有 ACK 状态，`Assigned → Running` 直接成立，无法表达"已派发未确认"；ACK 语义需要在 domain 合同、wire DTO 和状态机中补齐后才能宣称 `code_enforced`。

### 9.3 CapabilityExecution

```text
Requested → PolicyChecked
PolicyChecked → AwaitingApproval / Authorized / Denied
AwaitingApproval → Authorized / Denied / Cancelled / Unknown / Expired
Authorized → Dispatching → Executing
Executing → Succeeded / Failed / Cancelled / Unknown
```

终态：`Succeeded`、`Failed`、`Cancelled`、`Unknown`、`Denied`、`Expired`。`Expired` 与 §9.4 的 Approval `Expired` 对齐：Approval 一旦过期，对应 execution 必须进入 `Expired` 并 fail-closed，不得继续 dispatch 或原地等待。

> **开放决策（实现缺口）**：当前 `kiana-domain::CapabilityExecutionState` 没有 `Expired` 状态，过期只能表现为 `Denied` 或流程卡死；补齐前不得宣称过期审批已 fail-closed。

### 9.4 Approval

```text
Staged → Active
Active → Approved / Denied / Expired / Cancelled
Approved → Consumed
```

Approval 的 `Expired` 必须传播到对应 CapabilityExecution 的 `Expired`（§9.3）；`Approved` 只能被消费一次。

禁止：

- 过期 approval → execute；
- denied approval → execute；
- cancel_requested → completed；
- 未确认副作用 → completed；
- result_unknown 自动变成 success；
- Review 改写 Builder 原始事实。

---

## 10. 审批、取消与未知结果

### 10.1 推荐审批语义

Harness 遇到 `Ask` 时，不应把它伪装成普通 tool failure，而应创建：

```text
PendingInvocation
→ ApprovalChallenge
→ Run::AwaitingApproval
→ approve / deny / expire
→ 同一个 Runner continuation
```

Challenge 必须绑定：

- actor/session；
- exact payload digest；
- operation 和目标资源；
- policy version；
- budget；
- expiry；
- nonce；
- 单次消费状态。

Agent、Symposium、投票或自然语言报告不能替代 R3–R5 的人类确认。

> **开放决策（规范空白）**：当前 `kiana-entrypoints` 存在 `approve_local_write` 自动批准路径——该 app-state 标志为真且挑战风险为 `LocalWrite` 时，入口层直接以 `ApprovalDecision::Approve` 消费挑战，不经过人类确认。本规范未授权入口层自动批准，也未规定该路径的适用条件、可见性和审计要求；需在 ADR 中明确它是"用户预先声明的本地策略"还是应被移除。无论结论如何，都不得据此放宽 R3–R5 的人类确认要求。

### 10.2 取消 fencing

取消过程必须是：

```text
CancelRequested
→ authorization fence
→ dispatch fence
→ handler fence
→ runner stop
→ process-group/cgroup termination
→ result confirmation
```

如果无法确认副作用已经停止，必须停留在 `cancel_requested`（UI 投影名 `cancelling`，不是状态或终态）或进入 `result_unknown`，不能返回 completed。`cancelling` 不得作为 wire 状态或终态出现，唯一未知终态是 `result_unknown`。

### 10.3 Unknown

以下情况必须进入 `result_unknown`：

- handler 已开始但结果事件丢失；
- provider timeout，无法确认是否成功；
- 进程崩溃发生在副作用前后不明处；
- 外部订单/支付/消息状态无法核验；
- event/receipt 持久化失败；
- 取消已返回但子进程仍不确定。

Unknown 不得自动盲重试。补偿、退款或取消本身也是新的副作用，需要新的授权。

---

## 11. 事实源、事件和 Receipt

目标模型：

```text
Event Store      不可变事实源
State Store      当前状态与索引
Artifact Store   持久工件
Receipt          事实投影
Transcript       可丢弃 UI 视图
```

事件最低字段：

```text
event_id
aggregate_type
aggregate_id
stream_version
correlation_id
causation_id
request_id
session_id
run_id
turn_id
cell_id
work_packet_id
execution_id
invocation_id
actor_id
organization_id
department_id
idempotency_key
occurred_at
schema_version
payload
```

每次重试产生新的 `execution_id`，`invocation_id` 保持不变；重试、continue、审批续跑和 reconciliation 都通过这组 ID 关联到同一逻辑调用。

必须支持 aggregate stream、expected version/CAS 和幂等 append。当前仅按 request 的 process-local JSONL 不能证明跨进程恢复。

Receipt 至少要回答：

- 谁发起；
- 哪个组织、项目、工作区；
- 哪个角色、Cell 和 WorkPacket；
- 哪些能力被请求、允许、拒绝或等待；
- 哪些文件和外部资源改变；
- 哪些证据已持久化；
- 是否执行了测试或独立 Review；
- 是否存在例外、取消或 Unknown。

Receipt 证明记录了什么，不自动证明现实世界一定发生了什么。

---

## 12. 安全宪法

本节是安全条款的总纲摘要，canonical 条款以 [`company-os-security-constitution.md`](company-os-security-constitution.md) 的 SEC-01–SEC-12 为准；冲突时以安全宪法为准。

1. 所有外部和物理效果只能由 ControlPlane 服务端身份、短期 exact Grant、最终 payload digest、有效审批、预算和幂等约束后经 Broker 执行。
2. 模型、文本、网页、Provider、Artifact、工具描述和普通消息永不授予权限。
3. Grant、Role、Packet、Approval、Receipt、责任链不可由 Agent 修改或转移。
4. 子权限只能缩小，不能 union、提升或绕过父级限制。
5. 拒绝、过期、撤销、超预算、依赖不满足、校验失败和 Unknown 一律 fail-closed。
6. Secret 原值只在受控 Broker 的单次 invocation 中解析，不进入 prompt、transcript、event、Receipt、stdout、argv 或错误消息。
7. R3+ 单次确认，R4 二次确认，R5 默认禁止自治并要求独立物理安全。
8. crash、restart、timeout、cancel 和 partial effect 必须可观测、可对账，不得假成功。
9. 未有真实 adapter、cassette、测试和证据前，只宣称 `local_behavior`。
10. 每个新增能力先证明 deny、unknown、replay、TOCTOU、越权、注入、泄漏和恢复，再证明 happy path。

与 canonical 条款的对应关系：

| 本文条款 | 对应 canonical 条款 |
|---|---|
| 1 | SEC-04（外部/物理效果）、SEC-01（服务端身份与绑定） |
| 2 | SEC-11（不可信输入） |
| 3 | SEC-02（权限单调缩减） |
| 4 | SEC-02 |
| 5 | SEC-08（cancel fencing）、SEC-09（Unknown 一等状态） |
| 6 | SEC-05（Secret 不出 Broker） |
| 7 | SEC-04 |
| 8 | SEC-08、SEC-09 |
| 9 | 安全宪法 §1 证明等级（无独立 SEC 编号） |
| 10 | SEC-07（路径与 TOCTOU）、SEC-11、SEC-12（资源耗尽） |

Loopback 只是监听范围，不是认证边界（SEC-06）。近期应加入 per-instance bearer 或受保护 Unix socket、Host/Origin 校验和显式 session ownership。

---

## 13. Rust 实现映射

### 当前主路径（冻结）

```text
kiana-entrypoints
  → kiana-client / kiana-protocol
  → kiana-daemon::DaemonHost
  → kiana-core::ControlPlane
  → kiana-policy + kiana-gates + approvals
  → kiana-runner::KianaHarness
  → kiana-capability-broker
  → handlers
```

### 推荐增量模块

| 能力 | 推荐位置 |
|---|---|
| 组织、Agent、Cell、状态机 | `kiana-domain` |
| versioned DTO 和命令 | `kiana-protocol` |
| Organization/Agent/Packet/Communication/Receipt ports | `kiana-ports` |
| spawn/delegate/supervise/merge/retire | `kiana-core` |
| 组合、目录、调度、投影 | `kiana-daemon` |
| Agent runtime | `kiana-runner` |
| 事件持久化与恢复 | `kiana-eventlog` + 新 ports |
| capability descriptor/dispatch | `kiana-capability-broker` |
| coding adapter | 复用现有 shell/patch/query/MCP |
| office/life/travel/IoT | 独立 adapter crate/bounded context |

`kiana-runner` 不直接执行新能力；新 Company OS 不得绕过 ControlPlane。

旧 `kiana-tools`、旧 SDK、旧 TUI/runner 保持明确 compatibility boundary，禁止新能力回流旧 executor。

---

## 14. 实施路线图

> 全局阶段序列以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 为唯一 canonical；本节不另行定义 P 编号。以下按主题登记本文档负责的设计与实施要点，并标注其对应的 canonical 阶段。

| 本节主题 | canonical 阶段（spec-index §7） |
|---|---|
| 证据基线 | P0 |
| 控制面加固 | P0（Identity、Runtime ledger、Event/Receipt 和负向测试） |
| 持久证据 | P2（Durable Workflow、Recovery、Artifact、Client cursor） |
| 受控工作流 | P3（Objective → Project → Packet → Builder → Review → Acceptance → Close） |
| Coding Pack | P1（Capability Catalog）+ P0 事实基线 |
| Office / Search / Life Adapters | P5 |
| Team / Remote / Enterprise | P6 |

当前进度：S0 Gate 0 对 2026-09-07 快照为绿（见 `CURRENT_STATUS.md` 的 `Current Gate 0 revalidation after MCP, Swarm, and JSONL recovery slices` 与 `Gate 0 full regression after Web listener and bearer/session slices` 证据块）；工作重点处于 P1 安全加固切片，P1-01（authenticated principal / Web listener）、P1-02（approval continuation）、P1-03（cancel fencing）等均为 `partial`，P2 recovery 仍为 `partial`，S1–S4 未提升。以下内容仍是规范目标，不得据此宣称当前能力。

### 14.1 证据基线（canonical P0）

目标：让文档、实现和测试不再互相矛盾。

任务：

- 固定 clean baseline 与 dirty WIP；
- 记录 commit、环境、命令和结果；
- 修复或隔离当前 workspace test 失败；
- 修复 dependency boundary；
- 将 workbench smoke 接入正式 gate；
- 清理通用 `AGENTS.md` 中错误 npm 命令；
- 建立 source / enforcement / test evidence 三列账本。

验收：

```bash
cargo fmt --all --check
cargo check --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline --no-fail-fast
bash scripts/release-smoke.sh
bash scripts/v10-workbench-smoke.sh
```

不做：新 Provider、新 MCP transport、新生活 adapter、新 UI 大功能。

### 14.2 控制面加固（canonical P0）

任务：

- 修复失败/取消后的 stale run；
- Web API 显式 `session_id`，去掉全局 active；
- 统一锁顺序；
- 错误码和 typed state；
- cancel fence 和子进程管理；
- path TOCTOU 防护；
- loopback token/Origin/Host；
- body、turn、event、output、并发 quota；
- 标记并隔离 legacy runtime。

验收：未信任、越界、错误 role、错误 sandbox、过期 approval 不产生写盘或外部副作用。

### 14.3 持久证据（canonical P2）

任务：

- aggregate event stream；
- Session/Run/Approval/Artifact/PathLock/Budget ports；
- PendingInvocation；
- crash-consistent append；
- Receipt 从事件重建；
- `result_unknown` 和 reconciliation；
- kill-9、磁盘满、半写 JSONL 测试。

不做：跨机器恢复、云同步、企业租户。

### 14.4 受控工作流（canonical P3）

跑通：

```text
Sponsor → Planner → WorkPacket → fresh Builder
→ independent Reviewer → deterministic verification
→ MergeReceipt → Acceptance → Delivery → Closer → ClosingReceipt
```

验收：Builder 不参加 planning/monitoring symposium；Reviewer 与 author 不同；packet、review、Acceptance 和 Receipt 可以重建。

### 14.5 Coding Pack（canonical P1 + P0）

对每个能力登记：问题、owner crate、风险、source evidence、local evidence、失败码、许可证、版本和测试。

### 14.6 Office / Search / Life Adapters（canonical P5）

先做只读和草稿能力，再做外部副作用。支付、预订和 IoT 必须单独安全评审，不能共享 coding 的宽泛 shell 权限。

### 14.7 Team / Remote / Enterprise（canonical P6）

只有 canonical P0–P5 连续绿色后才开始。必须重新设计身份、租户、RBAC、网络、密钥、审计、留存、远程 worker 和故障恢复。

---

## 15. 90 天 Backlog

> 本节是工程排序目标，不是当前状态。实际进度以 `CURRENT_STATUS.md` 为准：2026-09-07 Gate 0 回归为绿，P1-01/P1-02/P1-03 等安全切片与 P2 recovery 仍为 `partial`。

### 第 1–14 天：证据基线

- [ ] clean/WIP snapshot；
- [ ] workspace gate 失败分类；
- [ ] 修复 dependency boundary；
- [ ] smoke gate 接入 release；
- [ ] 统一文档当前声明；
- [ ] 生成 Evidence Ledger。

### 第 15–30 天：契约与状态

- [ ] ID registry；
- [ ] domain/protocol/error/event/receipt schema registry（目标目录见 `docs/schemas/README.md` §3）；
- [ ] Run/Turn/Invocation/Approval 状态机；
- [ ] duplicate request 语义；
- [ ] cancel 线性化点；
- [ ] Web session ownership。

### 第 31–60 天：Company OS 核心

- [ ] AgentTemplate；
- [ ] AgentInstance/Cell；
- [ ] SpawnPlan；
- [ ] BudgetLease；
- [ ] CapabilityGrant；
- [ ] SupervisionLease；
- [ ] DelegationPacket；
- [ ] MergeReceipt；
- [ ] RetirementRecord；
- [ ] canonical domain WorkPacket；
- [ ] legacy adapter boundary。

### 第 61–75 天：持久化与恢复

- [ ] aggregate event stream；
- [ ] durable state/lock/artifact ports；
- [ ] PendingInvocation；
- [ ] approval continuation；
- [ ] crash/failure injection；
- [ ] result reconciliation。

### 第 76–90 天：第一个完整闭环

- [ ] 一个 Planner 模板；
- [ ] 一个 Builder 模板；
- [ ] 一个 Reviewer 模板；
- [ ] 一个 Closer 流程；
- [ ] 一条固定 cassette 黄金路径；
- [ ] 一条拒绝路径；
- [ ] 一条取消路径；
- [ ] 一条 Unknown/恢复路径；
- [ ] 完整 WorkPacket、Review、MergeReceipt、ClosingReceipt。

---

## 16. 开放决策

以下决策必须在进入相应 canonical 阶段（[`company-os-spec-index.md`](company-os-spec-index.md) §7）前形成 ADR：

1. Event sequence 采用 per-aggregate、per-run 还是双层 cursor；
2. Approval 是暂停 Runner，还是统一作为 blocked；推荐暂停 Runner（已落地 same-host 暂停与续跑，证据见 `CURRENT_STATUS.md` 2026-08-29 P1-02 条目；剩余开放部分是 durable PendingInvocation 与跨进程恢复）；
3. Event Store 与 State Store 的事务边界；
4. handler 已执行但 event 未落盘的 reconciliation 方式；
5. 本地 loopback 使用 bearer token 还是 Unix socket（bearer token + Host/Origin 校验路径已落地，证据见 `CURRENT_STATUS.md` 2026-09-07 P1-01 `Web exact-listener Host/Origin denial` 条目；剩余开放部分是 durable authenticated principal、session ownership 与 Unix socket 方案取舍）；
6. PathLock 的持久化和 fencing 实现；
7. `kiana-tasks` WorkPacket 的迁移和兼容窗口；
8. 旧 TUI/SDK 的冻结或移除日期；
9. 外部 adapter 的供应商、数据处理和许可证；
10. Web 异步运行和 streaming 是否需要新的 protocol version。

---

## 17. 最终产品叙事

对个人用户：

> **把项目和工作交给 Agent，但不把权限、预算和最终决定权交出去。**

对工程负责人：

> **让工作从“聊天里说完成了”变成“有目标、有分工、有授权、有验收、有收据的交付”。**

对长期 Company OS 愿景：

> **Kiana 不是让 Agent 更像人，而是让工作像一家可靠的软件公司一样被定义、授权、执行、检查、记录和持续改进。**

当前版本的诚实描述仍然是：

> **Kiana 已形成面向本地行为证明的控制平面主路径；Company OS 的组织、细胞分裂、生活能力和团队协作仍是分阶段建设的规范目标，而不是当前已完成能力。**
