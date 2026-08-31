# Kiana Company OS 总体设计与实施规范

> 文档状态：Draft / Implementation Baseline
> 适用范围：Kiana 本地控制面、个人 Company OS、未来团队与生活能力域
> 领域合同补充：[`company-os-domain-contracts.md`](company-os-domain-contracts.md)
> 平台能力补充：[`company-os-platform-architecture.md`](company-os-platform-architecture.md)
> 证据上限：当前 checkout 只能宣称 `local_behavior`

> **本文速览（导读，非规范）**
>
> - **讲什么**：CompanyOS 的总蓝图——产品定位与北极星、治理/劳动/能力三个层次、五个 PMP 部门、核心对象词典（WorkPacket、Cell、Grant、Receipt…）、细胞分裂的防失控规则、R0–R5 风险分级、关键状态机、安全宪法总纲和分阶段路线图。
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

| 文档 | 权威职责 |
|---|---|
| `CLAUDE.md` | 当前架构约束、命令和协作规则 |
| `README.md` / `USER.md` | 当前用户面与可运行证据 |
| `DESIGN.md` | 版本决策和架构方向 |
| `PROCESS.md` | 证据、发布和工作流程 |
| `PHASES.md` | 阶段剧本和验收 |
| `COMPANY.md` | Company OS 愿景和组织模型 |
| 本文档 | Company OS 规范目标、状态契约、迁移和实施 backlog |
| `AGENTS.md` | Agent 协作附录；通用模板命令不作为 Kiana 事实 |

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
- **Project**：有范围、成功标准、工作区和生命周期的交付单元。

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
  parent_packet_id?
  from_department
  to_department
  owner_cell_id
  acceptor_id
  objective
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

必须指定 domain WorkPacket 为 canonical 类型。`kiana-tasks` 等旧类型只能作为调度投影或兼容 adapter，不得形成第二个真相。

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

### 6.1 Boss 的决策步骤

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
→ ClosingReceipt
```

部门、角色、收件人和 `from/to` 字段都是声明，不能代替真实的服务端 authority chain。

---

## 8. 能力域与风险分级

统一采用 R0–R5，并按副作用、敏感性、资金、法律、物理危险和可逆性正交计算，取最高风险。

| 等级 | 定义 | 示例 | 授权要求 |
|---|---|---|---|
| R0 | 只读/观察 | 读代码、搜索、查路线、读设备状态 | 最小读取 scope |
| R1 | 可逆本地写入 | 改文件、生成草稿、整理笔记 | 路径/资源授权，快照或 undo |
| R2 | 个人事务/对外草拟 | 邮件草稿、预填表单、行程草稿 | 显示草稿和提交目标 |
| R3 | 外部非金钱副作用 | 发消息、提交工单、发布内容 | 最终 payload 单次确认 |
| R4 | 金钱/预订/法律承诺 | 支付、下单、订票、订房、签约 | 二次确认、预算和幂等 |
| R5 | 物理/高后果动作 | 解锁、驾驶、燃气、高功率、医疗设备 | 默认禁止自治、人在回路 |

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

### 9.1 Cell

```text
Proposed → Validated → Spawning → Ready → Running
Running → WaitingInput / Blocked / Checkpointing / ReadyToMerge
Running → CancelRequested → Cancelled
Running → Stalled → Retrying → Failed → Quarantined
ReadyToMerge → Merging → Succeeded → Retiring → Retired
```

### 9.2 WorkPacket

```text
Draft → Approved → Assigned → Running
Running → Blocked / AwaitingApproval / Failed / Cancelled
Running → Succeeded → Reviewed → Closed
```

### 9.3 CapabilityExecution

```text
Requested → PolicyChecked
PolicyChecked → AwaitingApproval / Authorized / Denied
Authorized → Dispatching → Executing
Executing → Succeeded / Failed / Cancelled / Unknown
```

### 9.4 Approval

```text
Staged → Active
Active → Approved / Denied / Expired / Cancelled
Approved → Consumed
```

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

如果无法确认副作用已经停止，终态必须是 `cancelling` 或 `result_unknown`，不能返回 completed。

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
actor_id
organization_id
department_id
idempotency_key
occurred_at
schema_version
payload
```

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

Loopback 只是监听范围，不是认证边界。近期应加入 per-instance bearer 或受保护 Unix socket、Host/Origin 校验和显式 session ownership。

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

### P0：Evidence Baseline

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

### P1：Core Hardening

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

### P2：Durable Evidence

任务：

- aggregate event stream；
- Session/Run/Approval/Artifact/PathLock/Budget ports；
- PendingInvocation；
- crash-consistent append；
- Receipt 从事件重建；
- `result_unknown` 和 reconciliation；
- kill-9、磁盘满、半写 JSONL 测试。

不做：跨机器恢复、云同步、企业租户。

### P3：Controlled Workflow

跑通：

```text
Sponsor → Planner → WorkPacket → fresh Builder
→ independent Reviewer → deterministic verification
→ MergeReceipt → Closer → ClosingReceipt
```

验收：Builder 不参加 planning/monitoring symposium；Reviewer 与 author 不同；packet、review 和 Receipt 可以重建。

### P4：Coding Pack

对每个能力登记：问题、owner crate、风险、source evidence、local evidence、失败码、许可证、版本和测试。

### P5：Office / Search / Life Adapters

先做只读和草稿能力，再做外部副作用。支付、预订和 IoT 必须单独安全评审，不能共享 coding 的宽泛 shell 权限。

### P6：Team / Remote / Enterprise

只有 P0–P5 连续绿色后才开始。必须重新设计身份、租户、RBAC、网络、密钥、审计、留存、远程 worker 和故障恢复。

---

## 15. 90 天 Backlog

### 第 1–14 天：证据基线

- [ ] clean/WIP snapshot；
- [ ] workspace gate 失败分类；
- [ ] 修复 dependency boundary；
- [ ] smoke gate 接入 release；
- [ ] 统一文档当前声明；
- [ ] 生成 Evidence Ledger。

### 第 15–30 天：契约与状态

- [ ] ID registry；
- [ ] domain/protocol/error/event/receipt schema registry；
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

以下决策必须在进入 P1/P2 前形成 ADR：

1. Event sequence 采用 per-aggregate、per-run 还是双层 cursor；
2. Approval 是暂停 Runner，还是统一作为 blocked；推荐暂停 Runner；
3. Event Store 与 State Store 的事务边界；
4. handler 已执行但 event 未落盘的 reconciliation 方式；
5. 本地 loopback 使用 bearer token 还是 Unix socket；
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
