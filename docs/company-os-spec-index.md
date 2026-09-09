# Kiana Company OS 规范索引与合同注册表

> 本文件是 CompanyOS 规范、合同和 owner 的索引，不重复定义领域字段、状态机或安全条款。
>
> 当前行为、当前版本和当前证据以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；规范目标不等于已实现能力。

> **本文速览（导读，非规范）**
>
> - **讲什么**：全部规范的"注册表"——每个概念归哪份文档和哪个 crate 管（canonical owner）、文档权威层级、入口与依赖边界、schema 版本与迁移规则、实施切片总览和文档变更卡模板；§4.4 汇总本轮新增的 canonical 合同（运行恢复与投影、调用账本、重试超时、检查点、审批决定、委派失败、工件图、记忆准入、扩展声明）。
> - **回答的问题**："我要新增或修改一个概念，它的正式定义在哪、要同步更新哪些合同和测试。"
> - **什么时候读**：写代码或改文档之前查表用；它是索引，不适合从头读到尾。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典。

## 1. 规范用途

本索引回答三个问题：

1. 某个概念由哪份文档和哪个 crate 负责；
2. 某项能力属于当前事实、规范目标还是实施计划；
3. 新增或修改能力时，需要更新哪些合同、事件、测试和证据。

完整阅读导航见 [`README.md`](README.md)。

## 2. 文档权威层级

```text
源码 + 精确测试/Smoke 回执 + CURRENT_STATUS.md
  → README.md / USER.md 当前用户面
  → 安全宪法 + 已批准 ADR
  → CompanyOS 领域/平台规范
  → 实施大纲 + PHASES.md + PROCESS.md + DESIGN.md
  → reference-agent-audit/ 和 reference/ 审计材料
```

| 问题 | 首要依据 | 不能从哪里推断 |
|---|---|---|
| 当前能运行什么 | `CURRENT_STATUS.md`、`README.md`、`USER.md`、精确回执 | 不能从目标规范推断 |
| 术语怎么用大白话理解 | `company-os-overview.md`（非规范导读） | 不能作为规范语义、canonical 定义或当前状态依据 |
| CompanyOS 业务是什么 | `company-os-domain-contracts.md` | 不能从工具名或 prompt 推断 |
| Agent 平台怎么协作 | `company-os-platform-architecture.md` | 不能从某个旧 runtime 推断 |
| 如何保障长期运行 | `company-os-operations-governance.md` | 不能从一次成功运行推断 |
| 如何评测和扩展 | `company-os-quality-ecosystem.md` | 不能从代码存在推断质量 |
| Coding pack 哪些行为完成或冻结 | `coding-pack-matrix.md`、精确回执 | 不能从"已绿"外推 durable/live，也不能反推表外能力 |
| 安全边界是什么 | `company-os-security-constitution.md` | 不能由模型文本放宽 |
| 先实施什么 | `company-os-implementation-outline.md`、`PHASES.md` | 不能因参考项目有此功能就提前打开 |
| 参考项目学什么 | `company-os-reference-matrix.md`、`reference-agent-audit/` | 不能把参考代码当 Kiana 证据 |

若文档冲突，先记录源码快照和冲突，再修改声明；不得通过放宽测试断言消除冲突。

## 3. CompanyOS 文档地图

| 层 | 文档 | 责任 |
|---|---|---|
| 总入口 | [`README.md`](README.md) | 阅读路径、权威层级和变更规则 |
| 白话导读 | [`company-os-overview.md`](company-os-overview.md) | 非规范第一入口：心智模型、任务走读、术语词典、FAQ |
| 当前事实 | [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) | 当前状态、证明等级、Gate 结果和限制 |
| 产品总规范 | [`company-os-design.md`](company-os-design.md) | 北极星、PMP、控制面、能力风险和产品边界 |
| 业务领域 | [`company-os-domain-contracts.md`](company-os-domain-contracts.md) | Objective、Project、Acceptance、Delivery、Outcome 等业务事实 |
| Agent 平台 | [`company-os-platform-architecture.md`](company-os-platform-architecture.md) | Runtime、Memory、Context/Cache、MCP、Workflow、Swarm、Provider |
| 运营治理 | [`company-os-operations-governance.md`](company-os-operations-governance.md) | Identity、Trigger、Human Inbox、Artifact、Cost、Recovery、Data Governance |
| 质量生态 | [`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md) | Eval、GoldenTrace、Feedback、Code Intelligence、Plugin、Supply Chain |
| 用户体验 | [`company-os-ui-ux.md`](company-os-ui-ux.md) | CLI/TTY、Web Workbench、Desktop、Run/Approval/Receipt/Recovery 投影 |
| 安全证据 | [`company-os-security-constitution.md`](company-os-security-constitution.md) | 安全宪法、风险等级、负向验收和发布门 |
| 工程实施 | [`company-os-implementation-outline.md`](company-os-implementation-outline.md) | Slice、owner、测试、证据和 90 天顺序 |
| Coding 审计 | [`coding-pack-matrix.md`](coding-pack-matrix.md) | v1.0 Coding pack 审计底表与冻结清单权威（不在表内、没有 owner 和证据的能力不算完成） |
| 参考速查 | [`company-os-reference-matrix.md`](company-os-reference-matrix.md) | 功能与参考项目的映射、吸收和排除 |
| 机器合同 | [`schemas/README.md`](schemas/README.md) | schema 分层、版本、校验和迁移规则 |
| 参考审计 | [`reference-agent-audit/README.md`](reference-agent-audit/README.md) | 外部项目审计和证据索引 |

## 4. Canonical owner 注册表

### 4.1 Company 业务域

| 合同 | 核心内容 | canonical owner | 运行时/投影 |
|---|---|---|---|
| `Organization` | 组织根、成员和数据边界 | `kiana-domain` | `kiana-core` / `kiana-daemon` |
| `Objective` | 指标、基线、目标、时间窗和 owner | `kiana-domain` | `kiana-daemon` projection |
| `Initiative` | 问题、假设、价值和立项决策 | `kiana-domain` | `kiana-core` |
| `Portfolio` / `Program` | 投资组合、共享目标和依赖 | `kiana-domain` | future projection |
| `Project` | 范围、成功标准、预算和生命周期 | `kiana-domain` | `kiana-core` / `kiana-daemon` |
| `Charter` / `Plan` | 项目立项工件与版本化计划（`Project.charter_ref` / `plan_ref`） | `kiana-domain` | `kiana-core` |
| `Milestone` | 阶段交付和验收标准 | `kiana-domain` | `kiana-daemon` |
| `WorkPacket` | 跨部门目标、写集、依赖、验收和责任 | `kiana-domain` | `kiana-core` |
| `Acceptance` | criteria snapshot、Evidence 和独立决策 | `kiana-domain` | `kiana-core` |
| `Delivery` | 版本化产物、接收方和交付确认 | `kiana-domain` | `kiana-eventlog` |
| `Outcome` | 测量窗口、指标观测和结果实现程度 | `kiana-domain` | `kiana-daemon` projection |
| `ChangeRequest` | 范围、预算、期限和验收的版本变更 | `kiana-domain` | `kiana-core` |
| `Risk` / `Incident` | 未发生风险与已发生异常 | `kiana-domain` | `kiana-eventlog` / operations |

详见 [`company-os-domain-contracts.md`](company-os-domain-contracts.md)。该文档定义业务语义；本索引不复制其字段和状态机。

### 4.2 Agent 和 Runtime 域

| 合同 | 核心内容 | canonical owner | 运行时/投影 |
|---|---|---|---|
| `Department` / `RoleSpec` | 部门使命、角色工具和知识 ACL | `kiana-domain` | `kiana-core` |
| `AgentTemplate` | 固定 prompt、工具、sandbox、预算和生命周期策略 | `kiana-domain` | `kiana-daemon` |
| `AgentInstance` / `Cell` | 有界执行单元、parent、grant、budget 和 supervision | `kiana-domain` | `kiana-core` / `kiana-daemon` |
| `SpawnPlan` / `DelegationPacket` / `SpawnGrant` | 分裂申请、父子 Cell 运行时授权信封和 spawn 能力特化视图 | `kiana-domain` | `kiana-core` |
| `Session` | 长期会话、owner、workspace 和 lineage | `kiana-domain` | `kiana-runner` / event store |
| `Turn` / `Run` | 交互轮次和执行生命周期 | `kiana-domain` | `kiana-runner` |
| `Invocation` / `CapabilityExecution` | 工具调用、审批、执行、结果和 Unknown | `kiana-domain` | broker / `kiana-core` |
| `RuntimeEvent` | 不可变执行事实和 cursor | `kiana-domain` / event ports | `kiana-eventlog` |
| `Artifact` / `Receipt` | 产物和事实投影 | `kiana-domain` / event ports | `kiana-eventlog` / daemon |
| `HandoffReceipt` / `MergeReceipt` / `RetirementRecord` / `ClosingReceipt` | 交接 ACK、合并、退役和项目关闭的控制面收据 | `kiana-domain` | `kiana-core` / `kiana-eventlog` |
| `BudgetLease` | token、工具、墙钟、并发和 effect 配额（canonical 名；旧 `CellBudget` 已废弃） | `kiana-domain` | `kiana-core` |
| `ProviderBudget` | Provider 速率、并发和费用限制层（operations 扩展层，横切而不是 `BudgetLease` 的子级） | `kiana-core` / provider adapter | `kiana-core` |
| `RateCard` | 版本化单价表；`UsageRecord.estimated_cost` 必须引用其 version，价格变化只能新增版本 | `kiana-core` / `kiana-eventlog` | `kiana-core` |
| `CapabilityGrant` | 能力、资源、路径、TTL 和 approval scope | `kiana-domain` | `kiana-core` / broker |
| `SupervisionLease` | heartbeat、checkpoint、stall 和 retry 限制 | `kiana-domain` | `kiana-core` |

### 4.3 平台合同

| 平台能力 | canonical 合同 | 主要 owner |
|---|---|---|
| Context / Memory | `ContextPlan`、`MemoryRecord`、`MemoryQuery`、`ContextCheckpoint` | `kiana-query` / ports |
| Capability / MCP | `CapabilityDescriptor`、`ToolSnapshot`、`McpServerDescriptor` | broker / `kiana-daemon` |
| Workflow | `WorkflowDefinition`、`WorkflowInstance`、`NodeExecution`、`Signal` | `kiana-workflow` / `kiana-core` |
| Swarm | `SwarmPlan`、`Partition`、child Cell、`MergeDecision` | `kiana-core` / `kiana-daemon` |
| Provider | `NormalizedEvent`、usage、cursor、terminal event | provider adapter / protocol |
| Operations | `Principal`、`AuthorityEpoch`、`TriggerDefinition`、`HumanTask`、`RecoveryPlan` | `kiana-domain` / `kiana-core` / `kiana-daemon` / ports |
| Cost / Capacity | `UsageRecord`、`CostLedger`、`CostCorrection`、`Quota`、`Backpressure` | `kiana-core` / `kiana-eventlog` |
| Data Governance | `DataClass`、`Purpose`、`ProcessingGrant`、`Retention`、`SharingGrant` | `kiana-domain` / `kiana-ports` |
| Connector | `ConnectorDefinition`、`AccountBinding`、`ProviderReceipt` | broker / adapter crates |
| Quality | `EvalSuite`、`EvalCase`、`GoldenTrace`、`QualityGate` | runner / eventlog |
| Ecosystem | `ExtensionManifest`、Skill/Workflow/Capability Pack | skills / broker / daemon |
| UI/UX | `UiSnapshot`、`UiAction`、cursor、epoch、pending action | `kiana-protocol`（wire DTO）/ `kiana-daemon`（投影生成） |

详见 [`company-os-platform-architecture.md`](company-os-platform-architecture.md)、[`company-os-operations-governance.md`](company-os-operations-governance.md)、[`company-os-ui-ux.md`](company-os-ui-ux.md) 和 [`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md)。

### 4.4 本轮新增 canonical 合同（2026-09-08）

本节登记本轮规范增量。字段和状态机不在本索引重复，以各 canonical 文档为准；`deferred` 是第二阶段目标，不属于当前实施范围。

| 合同 | 核心内容 | canonical owner | 运行时 owner | 状态 |
|---|---|---|---|---|
| `RunSnapshot` | 可序列化 durable pause / resume 边界（`run_id` / `session_id`、消息历史、`pending_capability{call_id, args_fingerprint}`、`approval_decisions`、`step_counter`、`schema_version`）；ControlPlane 独占写入，runner 不读盘恢复 | `kiana-domain` | `kiana-core`（写入 / `resume_run` 重建） | target |
| `RunProjection` / `InvocationProjection` | 折叠 `run.*` / `capability.*` / `approval.*` 事件，得到 Run / Invocation 当前状态、待审批集和未决 capability 集；内存表降级为写穿缓存，按 run_id / session 惰性重建 | `kiana-core` / `kiana-domain` | `kiana-eventlog` / `kiana-daemon` | target |
| 调用账本 `InvocationLedgerEntry` | 键为 `(run_id, call_id)`；dispatch 前查账，已 `executed` 返回缓存或 fail-closed，`args_fingerprint` 不符立即拒绝，`unknown` 不自动重试 | `kiana-domain` | `kiana-core`（独占写入）/ `kiana-eventlog` | target |
| `RetryPolicy` / `TimeoutPolicy` | 可序列化重试（`initial_interval` / `backoff_coefficient` / `maximum_attempts` / `non_retryable_error_types`）与超时（`start_to_close` / `schedule_to_close`）；默认单次尝试，非幂等禁止重试，超时收敛 `result_unknown` | `kiana-domain` | `kiana-core` / broker | target |
| `CheckpointService` / `Checkpoint` | 检查点绑定 transcript offset + workspace revision + `invocation_id`；写前快照，恢复走 ControlPlane 并作废旧 approval | `kiana-domain` | `kiana-core` / `kiana-daemon` | 状态层 + 编辑级 undo 为 target；文件层 shadow git 为 deferred |
| `ApprovalDecision` | 审批决定事件：actor / scope / subject / expiry_at / result / feedback / decided_at；`denied` ≠ `cancelled`，拒绝只拒当前 invocation | `kiana-domain` | `kiana-core`（追加事件）/ `kiana-eventlog` | 首发三动作 + `once` 为 target；`turn` 随 A-4（P1）落地；`session` / `policy` 作用域为 deferred |
| `DelegationPacket` 预算与终止字段 | `max_turns` / `max_messages` / `termination_predicate` / `handoff_allowlist`；委派是 ControlPlane 的 assign / handoff 操作，不是第六个模型工具 | `kiana-domain` | `kiana-core` | target |
| `ChildFailureReport` | typed 子失败信封（`child_cell_id` / `reason_code` / `error_class` / `partial_output_refs` / `result_unknown` / `retryable` / `policy_snapshot`）；`result_unknown` 的 child 输出不得进 MergeDecision | `kiana-domain` | `kiana-core` / `kiana-eventlog` | target |
| `ArtifactGraph` / `workflow validate` | 版本化工件图（`id` / `generates` / `template` / `requires`）；apply 只认 requires 传递闭包，缺依赖 blocked；`kiana workflow validate --json` 只读门禁 | `kiana-workflow` | `kiana-workflow` / `kiana-core` | target |
| `MemoryRecord` 扩展字段 | 服务端派生 `origin`、`review_state`、`admission_state`（candidate / qualified / ephemeral，与 `status` 正交）；instance-scratch 默认可见，持久层 candidate 默认不可检索 | `kiana-query` / ports | `kiana-query` / `kiana-daemon` | target |
| `ExtensionManifest` 扩展字段 | `effect`（read-only / read-write）、`required_capabilities`、`content_hash` / `signature`；声明 ≠ 授权，安装时校验 | `kiana-skills` / `kiana-types` | broker / `kiana-daemon` | target |

字段与状态机分别见 [`company-os-domain-contracts.md`](company-os-domain-contracts.md) §4.10–§4.13、[`company-os-platform-architecture.md`](company-os-platform-architecture.md) §4.1 / §5.3 / §8.2、[`company-os-design.md`](company-os-design.md) §10.4、[`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md) §9.1 / §9.5。

## 5. 入口与依赖边界

正式产品主路径固定为：

```text
CLI / TTY / Web / Desktop
  → kiana-protocol
  → kiana-daemon::DaemonHost
  → kiana-core::ControlPlane
  → Policy + Gates + Approval
  → Capability Broker / KianaHarness
  → handlers / EventStore / Receipt
```

依赖规则：

- `kiana-domain` 只定义稳定对象、值对象、不变量和状态；
- `kiana-protocol` 只定义 versioned wire DTO、命令、错误和事件载荷；
- `kiana-ports` 定义 core 与 adapter 的接口；
- `kiana-core` 执行 authority chain、policy、gate、approval、lifecycle 和 fencing；
- `kiana-daemon` 负责组合根、目录、调度、投影和本地 adapter；
- `kiana-runner` 负责规范 Agent loop，但不直接执行新能力；
- `kiana-eventlog` 负责事件事实、CAS、恢复接口和 Receipt 投影；
- `kiana-query` 负责上下文、索引、repo map、Memory 和查询；
- `kiana-capability-broker` 负责 capability descriptor、dispatch 和结果边界；
- `kiana-workflow` 负责确定性流程定义，不拥有绕过 ControlPlane 的执行权限；
- 旧 `kiana-tools`、旧 SDK、旧 TUI/runner 只能作为兼容边界，不成为新 CompanyOS 的第二执行脊柱。

## 6. 状态、证明和迁移

### 6.1 状态维度

```text
feature_status:
  implemented | partial | target | deferred | not_supported

proof_level:
  source | local_behavior | durable | live | physical
```

两者必须同时记录。`implemented` 不自动代表 `durable`；`local_behavior` 不自动代表 live；`not_supported` 必须在入口处拒绝或显示不可用。

### 6.2 Schema 和迁移

```text
canonical domain schema
  ↔ wire protocol schema
  ↔ runtime event schema
  ↔ projection / receipt schema
```

- 兼容字段增加可以升 minor；
- 删除、类型改变、required 语义改变或状态语义改变必须升 major；
- 破坏性变化必须有 upcaster、迁移窗口或明确拒绝；
- Unknown major fail-closed；
- 事件历史不能因当前代码升级而静默改写；
- WorkPacket、RiskLevel、Approval 和 EventLog 的 legacy 兼容边界必须显式记录。

机器合同详见 [`schemas/README.md`](schemas/README.md) §3（目标目录）和 §5（版本规则）。

### 6.3 关键兼容边界

- 当前 `kiana-domain::RiskLevel` 四档（`ReadOnly`、`LocalWrite`、`ExternalSideEffect`、`Critical`）与目标 R0–R5 六档的版本化映射是**开放决策**，映射表待定，本节不发明映射（目标风险矩阵见 [`company-os-security-constitution.md`](company-os-security-constitution.md) §3）；
- `WorkPacket`、`SpawnPlan` 与 `DelegationPacket` 不是同一个对象：业务交接、分裂申请和运行时授权信封不能互相替代；
- Prompt Cache 不能替代 Session/Event 持久化；
- Tool Search 不能替代 Policy/Approval；
- `BudgetLease` 不能替代 ProjectBudget 或 FinancialBudget；
- `CellBudget` 是 `BudgetLease` 的早期草稿名，已废弃，预算对象统一使用 canonical 名 `BudgetLease`；
- Receipt 不能单独证明现实世界效果；
- stdio MCP 是当前支持边界，HTTP MCP、远程执行和物理控制仍需单独安全设计。

## 7. 实施切片总览

本节是全局实施阶段序列（P0–P6）的唯一 canonical；其他文档引用阶段编号时以本节为准。

```text
A  Contract Registry
B  Formal State Machines
C  Organization and Cell
D  WorkPacket and Handoff
E  Communication and Accountability
F  Approval and PendingInvocation
G  Event/State/Artifact Recovery
H  Capability Descriptor and Broker
I  Company Business Lifecycle
J  Runtime/Memory/Context/Cache/Workflow/Swarm/Provider
K  Identity/Trigger/Human Inbox/Artifact/Cost/Recovery/Data
L  Eval/Feedback/Code Intelligence/Extension/Supply Chain
M  UI/UX and Client Projection
```

当前推荐顺序：

```text
P0 事实基线、Identity、Runtime ledger、Event/Receipt 和负向测试
  ↓
P1 ContextPlan、Memory、Cache telemetry、Capability Catalog、Cost、Eval
  ↓
P2 Durable Workflow、Human Inbox、Recovery、Artifact、Client cursor
  ↓
P3 Objective → Project → Packet → Builder → Review → Acceptance → Close
  ↓
P4 有界 Swarm、Scheduler、Provider streaming、Skill/Plugin ecosystem
  ↓
P5 Office / Commerce / Travel / IoT bounded contexts
  ↓
P6 Team / Remote / Enterprise
```

详细 owner、依赖、测试和退出条件见 [`company-os-implementation-outline.md`](company-os-implementation-outline.md)。

## 8. Reference 使用规则

参考项目只用于形成设计假设，不能作为 Kiana 证据。实现卡必须登记：

```text
reference_project
reference_audit/path
adopted_pattern
rejected_pattern
Kiana adaptation
security difference
local fixture
expected event sequence
proof_level
```

首选吸收顺序：

1. DeepSeek Harness、OpenCode、Goose、Crush：Runtime、Event、Approval、Cancel、Recovery；
2. Cline、Aider、Continue、Roo Code、Pi、Agno：Context、Memory、Cache、Checkpoint、Client replay；
3. Pydantic AI、CrewAI、Archon：typed Workflow、DAG、Human Gate；
4. AutoGen、Agency Swarm、MetaGPT、ChatDev：有界 Swarm、Role、Handoff 和 Symposium。

明确排除：

- 自由 `SendMessage` 总线；
- 无限 Swarm、无限 GroupChat 和无限 model loop；
- 共享 transcript 作为状态或权限；
- 未经 Broker/Policy 的 shell；
- 用 cache、Memory 或模型文字证明业务结果；
- 没有幂等、对账和人工安全控制的支付、IoT 和物理动作。

完整矩阵见 [`company-os-reference-matrix.md`](company-os-reference-matrix.md)。

## 9. 当前 Gate 0 和证据入口

Gate 0 必须绑定 worktree 状态、源码快照、工具链、精确命令和结果。当前是否通过只看 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md)，不能从规范文档或局部 Slice 状态推断。

```bash
cargo fmt --all --check
cargo check --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline --no-fail-fast
bash scripts/release-smoke.sh
bash scripts/v10-workbench-smoke.sh
```

测试和发布证据必须区分：

```text
source
  → contract/local_behavior
  → durable
  → live / physical
```

失败、拒绝、取消、Unknown、恢复、路径竞态、Secret redaction 和 schema compatibility 都必须有负向证据；通过一次 happy path 不能提升整体产品状态。

## 10. 文档变更卡

每次变更至少填写：

```text
Change:
Affected documents:
Canonical owner:
Runtime owner:
Schema/version:
States and legal transitions:
Events and ordering:
Stable errors:
Security invariants:
Migration/rollback:
Tests and exact commands:
Evidence artifact:
feature_status:
proof_level:
Known limitations:
```

变更完成前不得：

- 将 `target` 写成 `implemented`；
- 将局部测试写成 durable/live/physical 证明；
- 让入口层、模型、网页、MCP 或 Artifact 绕过 ControlPlane；
- 通过复制语义制造第二个 canonical owner；
- 自动 commit、push、merge、release 或删除工作树。
