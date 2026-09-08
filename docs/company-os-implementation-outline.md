# Kiana Company OS 实施大纲

> 文档性质：工程实施索引，不是当前能力声明。
> 总体规范：[`company-os-design.md`](company-os-design.md)
> 当前证据上限：`local_behavior`。

> **本文速览（导读，非规范）**
>
> - **讲什么**：把所有规范拆成可认领的工程切片——模块责任地图（哪个 crate 管什么）、切片 A 到 M 各自的目标/实现位置/验收条件、切片实现卡登记位置、能力域实施顺序、证据与测试模板、Gate 0 命令入口、高优先级负向证据清单和 90 天工程序列。
> - **回答的问题**："下一步该做什么、在哪个 crate 做、做到什么程度算过关。"
> - **什么时候读**：认领开发任务时——它是"规范"到"代码"之间的桥；§6.1 列出当前已知负向证据，§6 只登记 Gate 0 命令，当前是否通过以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典；当前实际能力以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准。

## 1. 使用方式

本文件把 Company OS 总体设计拆成可认领的工程切片。每个切片在进入实现前必须补齐：

- owner；
- 依赖；
- canonical 类型；
- 稳定错误码；
- 状态转移；
- 事件序列；
- 测试 fixture；
- 预期 Receipt；
- 失败、取消和 `result_unknown` 语义。

不得把“已有对象”“已有 crate”或“已有测试”直接当成安全强制证据。每项能力都要分别记录：

```text
设计意图 → 代码强制 → 测试证据 → 发布证据
```

## 2. 模块责任地图

| 主题 | canonical 所在 | 主要实现位置 | 第一批测试 |
|---|---|---|---|
| ID、角色、部门、状态值对象 | `kiana-domain` | `kiana-domain/src/` | domain unit/property |
| Company 业务事实与生命周期 | `kiana-domain` | `kiana-domain/src/`；投影在 `kiana-daemon` | domain/contract/integration |
| 平台能力合同与组合 | 分层 owner：`kiana-runner` / `kiana-query` / `kiana-capability-broker` / `kiana-workflow` / `kiana-eventlog` | 见 [`company-os-platform-architecture.md`](company-os-platform-architecture.md) | contract/replay/failure/e2e |
| 运营、治理与可靠性 | 分层 owner：`kiana-core` / `kiana-daemon` / `kiana-eventlog` / `kiana-ports` | 见 [`company-os-operations-governance.md`](company-os-operations-governance.md) | ownership/recovery/incident/data |
| 质量、学习与生态 | 分层 owner：`kiana-query` / `kiana-runner` / `kiana-capability-broker` / `kiana-workflow` | 见 [`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md) | eval/replay/compatibility/supply-chain |
| UI、交互与客户端投影 | 分层 owner：`kiana-protocol` / `kiana-daemon` / `kiana-entrypoints` / `contrib/desktop` | 见 [`company-os-ui-ux.md`](company-os-ui-ux.md) | projection/a11y/smoke/recovery |
| 外部 DTO 和命令 | `kiana-protocol` | `kiana-protocol/src/` | protocol contract |
| 组织、Agent、Packet、通信、收据端口 | `kiana-ports` | `kiana-ports/src/` | fake adapter contract |
| 编排、授权、spawn、delegate、supervise、merge、retire | `kiana-core` | `kiana-core/src/` | core integration |
| 组合根、目录、调度、投影 | `kiana-daemon` | `kiana-daemon/src/` | daemon integration |
| Agent model loop | `kiana-runner` | `kiana-runner/src/` | scripted runner |
| 事件事实与恢复 | `kiana-eventlog` | `kiana-eventlog/src/` | replay/crash tests |
| capability descriptor/dispatch | `kiana-capability-broker` | `kiana-capability-broker/src/` | broker contract |
| policy | `kiana-policy` | `kiana-policy/src/` | policy matrix |
| gate/approval mapping | `kiana-gates` | `kiana-gates/src/` | gate unit |
| context、memory、hooks | `kiana-query` | `kiana-query/src/` | ACL/query tests |
| CLI、TTY、Web、Desktop | `kiana-entrypoints` / `contrib/desktop` | entrypoint sources | HTTP/E2E/smoke |
| 长期流程和组织说明 | `COMPANY.md` / `PHASES.md` / `docs/` | documentation | docs consistency |

## 3. 规范对象实施顺序

### Slice A：契约注册表

**目标**：防止多个 crate 各自定义同名对象和状态。

**必须注册**：

- `request_id`、`session_id`、`run_id`、`turn_id`、`cell_id`、`work_packet_id`、`execution_id`、`invocation_id`、`approval_id`、`artifact_id`、`receipt_id`；
- domain、protocol、runner-event、artifact、receipt、error code schema；
- unknown field、unknown event、schema migration 规则。

**验收**：每个公开类型有唯一 owner、版本和转换测试；`kiana-tasks` 的 WorkPacket 明确降级为 projection/adapter，不形成第二真相。

### Slice B：正式状态机

**目标**：为 Request、Session、Run、Turn、Cell、WorkPacket、CapabilityExecution、Approval、Review、Artifact、PathLock 定义合法转移。

**必须冻结**：

- 终态不可回到运行态；
- cancel 与 dispatch 的线性化点；
- expired approval 不得执行；
- `result_unknown` 不得自动变为 success；
- `cancelled` 只能表示停止已确认；无法确认副作用是否停止时必须进入 `result_unknown`；
- Review 不得覆盖 Builder 原始事实；
- expected version/CAS 和重复事件语义。

统一状态表至少包含以下内容；实现时须进一步拆成逐状态命令表：

| Aggregate | 主要状态链 | 允许命令 | 前置不变量 | 关键事件 | 终态 |
|---|---|---|---|---|---|
| Cell | proposed → validated → spawning → ready → running → ready_to_merge → merging → succeeded → retiring → retired；另有 waiting_input、blocked、checkpointing、stalled、retrying、failed、quarantined、cancel_requested | validate、spawn、start、checkpoint、request_merge、cancel、retry、retire | 模板有效；深度/预算/并发未超限；Grant 只减不增；parent/root 不可变 | CellValidated、SpawnCommitted、CellStarted、Checkpointed、MergeRequested、CellCancelled、CellRetired | retired、cancelled、failed、quarantined |
| WorkPacket | draft → approved → assigned → running → succeeded → reviewed → closed；另有 blocked、awaiting_approval、failed、cancelled | approve、assign、accept、start、block、submit、review、close、cancel | 唯一 owner/acceptor；依赖满足；写集无冲突；ACK 后才转责 | PacketApproved、PacketAssigned、PacketAccepted、PacketBlocked、PacketSucceeded、PacketReviewed、PacketClosed | closed、failed、cancelled |
| CapabilityExecution | requested → policy_checked → awaiting_approval / authorized / denied → dispatching → executing → succeeded / failed / cancelled / unknown | evaluate、approve、deny、dispatch、complete、cancel、reconcile | authenticated context；exact Grant/payload digest；预算、TTL、幂等与 fence 有效 | CapabilityRequested、PolicyChecked、ApprovalRequired、CapabilityAuthorized、Dispatched、EffectRecorded、ResultUnknown | succeeded、failed、cancelled、unknown、denied |
| Approval | staged → active → approved / denied / expired / cancelled → consumed | activate、decide、expire、cancel、consume | actor/session/project/profile、payload digest、scope、nonce 和 expiry 匹配；只能消费一次 | ApprovalStaged、ApprovalActivated、ApprovalDecided、ApprovalExpired、ApprovalConsumed | consumed、denied、expired、cancelled |

错误与恢复契约必须为每个稳定错误码定义：CLI exit、HTTP status、是否可重试、是否需要新授权/补偿、Receipt 状态和事件。`result_unknown` 必须进入 reconciliation queue，不能自动 retry。

Schema 注册表必须区分 canonical domain schema 与 wire protocol schema；兼容字段增加可升 minor，破坏性变化必须升 major 并提供 upcaster/迁移，未知 major fail-closed。

**验收**：状态机 unit/property tests；非法转移、并发转移、重复请求、错误映射、Unknown 对账和 schema compatibility 均有断言。

### Slice C：组织与 Cell

**目标**：实现 `AgentTemplate`、`CellSpec`、`SpawnPlan`、`BudgetLease`、`CapabilityGrant`、`SupervisionLease`。

**实现位置**：优先 `kiana-domain` 定义契约，`kiana-core` 执行授权和生命周期，`kiana-daemon` 提供目录/调度。

**验收**：

- 模板版本固定；
- 子权限只能缩小；
- 默认不可再委派；
- root/parent/global 数量、深度、TTL、预算、并发和重试有限；
- 相同 WorkFingerprint 不重复创建；
- spawn 预留预算、锁和 grant 具有原子语义；
- retire 撤销 grant、释放锁和预算。

**当前状态**：`partial`（`local_behavior`）。CURRENT_STATUS 的「P3-01 Cell lifecycle admission evidence (2026-08-31)」记录 packet spawn 已执行 template/budget/grant/supervision/Cell 的 reserve→commit→terminal→retire 链；同一证据块记录 registry/budget 仍是进程内状态，尚无 durable Cell projector 和跨进程恢复。状态与证明等级以账本为准，不由此处推断。

### Slice D：WorkPacket 与交接

**目标**：把跨部门工作变成可独立授权、执行、验收和关闭的单位。

**必备字段**：目标、输入引用、依赖、写集、数据范围、验收测试、禁止事项、期限、预算和责任人。

**验收**：只有接收 ACK 后责任才转移；未响应不等于成功；packet 路径冲突和依赖缺失 fail-closed。

### Slice E：通信与问责

**目标**：区分 Chat、Command、Handoff、Decision、StatusReport、Evidence、Incident。

**实现位置**：`kiana-ports` 定义接口，`kiana-core` 产生正式事件，协议层传递 versioned DTO。

**验收**：

- 自由聊天不产生授权；
- Handoff 必须定向并 ACK；
- 每个任务有唯一 owner、acceptor、parent 和 escalation target；
- 每次状态变更都能沿 Sponsor → Project → Packet → Cell → Execution → Evidence → Receipt 回溯。

### Slice F：Approval 与 PendingInvocation

**目标**：让审批真正暂停并恢复同一个 Agent Run。

**推荐流程**：

```text
CapabilityRequest
  → ApprovalChallenge
  → PendingInvocation
  → Run::AwaitingApproval
  → approve / deny / expire
  → single-use consume
  → same Runner continuation
```

**验收**：approval 绑定 actor/session、精确 payload digest、目标资源、策略版本、TTL、nonce 和预算；并发批准、过期、取消和重放都有测试。

### Slice G：事实源与恢复

**目标**：建立 Event Store、State Store、Artifact Store 和 Receipt projection 的明确边界。

**事件最低字段**：

```text
event_id, aggregate_type, aggregate_id, stream_version,
correlation_id, causation_id, request_id, session_id, run_id,
turn_id, cell_id, work_packet_id, execution_id, actor_id,
organization_id, department_id, idempotency_key, occurred_at,
schema_version, payload
```

**验收**：

- aggregate stream 支持 expected version/CAS；
- crash/restart/kill-9 后可重建状态；
- handler 已执行但结果事件丢失进入 `result_unknown`；
- 损坏 JSONL、磁盘满和部分 artifact 不伪装成功；
- Web transcript 不是事实源。

### Slice H：Capability Descriptor 与 Broker

**目标**：将 `(CapabilityKind, operation)` 扩展为带域、版本、风险、scope、approval、幂等和补偿描述的 capability contract。

**命名建议**：

```text
office.calendar.read
travel.booking.quote
commerce.order.create
home.device.state.read
```

**验收**：未知 operation、参数越界、资源越界、过期 grant、超预算和 provider 不可用都 fail-closed；handler 返回结果必须经过 schema/provenance 校验。

### Slice I：Company 业务生命周期

**目标**：把 CompanyOS 从“受治理的 Agent 执行控制面”补成一条可追踪的本地交付闭环；不把业务对象实现成 prompt 中的名词，也不把 Agent 运行成功直接视为业务结果成功。

**规范来源**：[`company-os-domain-contracts.md`](company-os-domain-contracts.md)。本文只登记实施边界，字段、状态机和业务不变量以该规范为准。

**第一批 canonical 对象**：

- `Objective`：指标、基线、目标值、时间窗和负责人；
- `Initiative`：问题、假设、预期价值和立项决策；
- `Project`：范围基线、成功标准、非目标、里程碑和关闭条件；
- `Milestone`：阶段性交付和验收标准；
- `Acceptance`：冻结的 criteria snapshot、证据和独立决策；
- `Delivery`：交付产物、接收方和交付确认；
- `Outcome`：测量窗口、指标观测和结果实现程度；
- `ChangeRequest`、`Risk`、`Incident`：范围变更、尚未发生的风险和已发生的异常。

**实现位置**：

- `kiana-domain` 定义对象、ID、值对象、状态和不变量；
- `kiana-protocol` 定义 versioned 命令、事件和 DTO；
- `kiana-core` 执行 authority chain、命令守卫和状态转移；
- `kiana-eventlog` / `kiana-ports` 保存事实并支持投影；
- `kiana-daemon` 构建 Project/WorkPacket/Run 的本地组合和查询投影。

**第一条纵向切片**：

```text
Objective
  → Project Charter
  → Project Approval
  → Milestone + WorkPacket
  → Builder Run + Evidence
  → Independent Review
  → Acceptance
  → Delivery + ClosingReceipt
  → Outcome measurement
```

**必须先冻结的命令和事件**：

```text
propose_objective    → ObjectiveProposed
approve_project      → ProjectApproved
create_milestone     → MilestoneCreated
approve_packet       → PacketApproved
start_run            → RunStarted
request_acceptance   → AcceptanceRequested
decide_acceptance    → AcceptanceDecided
close_project        → ProjectClosed
record_outcome       → OutcomeRecorded
```

**不变量**：

- Project 未批准不能启动写盘或外部副作用；
- Project 关闭必须有 Acceptance、Delivery、ClosingReceipt，或有明确的失败关闭/人工豁免；
- Acceptance 使用验收标准快照，Reviewer 不得改写 Builder 原始事实；
- 范围、预算或验收标准变更必须通过 `ChangeRequest` 并产生新版本；
- `RuntimeBudget`、`ProjectBudget` 和未来的 `FinancialBudget` 不得混用；
- `result_unknown` 必须关联 Incident/Reconciliation，不能自动变成成功；
- Objective `Achieved` 必须有 Outcome 观测证据，不能由 Receipt 或模型文本直接宣称。

**验收**：

1. 新进程可以从事件和 Artifact 引用重建 Objective → Project → Packet → Run → Acceptance → Receipt；
2. 未批准、过期审批、越权路径、重复命令和重复交付均 fail-closed；
3. 拒绝、返工、暂停、取消、失败关闭和 Unknown 各有可重放状态；
4. Reviewer 与 Builder 不共享 author session 或可修改的验收基线；
5. 一个 fake-model Coding 项目能够产生完整的 ClosingReceipt 和未自动夸大的 Outcome 状态。

**当前状态**：`target`。现有 WorkPacket、Review、EventLog 和 Receipt 的局部实现不能替代完整 Company domain 聚合；状态提升必须绑定 `CURRENT_STATUS.md` 认可的源码快照、精确命令和证据。

### Slice J：平台能力平面

**目标**：把 Memory、Context、Cache、Capability Discovery、MCP、Workflow、Swarm、Provider 和 Observability 从散落实现提升为可认领的同核平台合同。

**规范来源**：[`company-os-platform-architecture.md`](company-os-platform-architecture.md)。本切片不新增第二套 ControlPlane 或 Agent loop。

| 子切片 | canonical 合同 | 主要 owner | 参考设计 | 当前状态 |
|---|---|---|---|---|
| J1 Runtime | Session、Turn、Run、Invocation、NormalizedEvent | `kiana-runner` / `kiana-eventlog` | DeepSeek Harness、OpenCode、Goose、Crush | `partial` |
| J2 Context/Cache | ContextPlan、TokenBudget、ContextCheckpoint、CacheTelemetry | `kiana-query` / provider adapter | Aider、Continue、Pi、Roo、Claude API prompt-cache pattern | `target` |
| J3 Memory | MemoryRecord、MemoryQuery、Promotion、Expiry、Provenance | `kiana-query` / `kiana-ports` | Continue、Letta、MemPalace、memorix | `partial` |
| J4 Capability/MCP | CapabilityDescriptor、ToolSnapshot、McpServer lifecycle | broker / `kiana-daemon` | DeepSeek ToolRuntime、OpenCode、Cline、Goose | `partial` |
| J5 Workflow | WorkflowDefinition、NodeExecution、Signal、Compensation | `kiana-workflow` / `kiana-core` | Pydantic AI Graph、CrewAI Flow、Archon YAML/DAG | `target` |
| J6 Swarm | SwarmPlan、Partition、Child Cell、MergeDecision | `kiana-core` / `kiana-daemon` | AutoGen、Agency Swarm、MetaGPT、ChatDev | `target` |
| J7 Provider/Output | provider-normalized stream、cursor、usage、terminal event | `kiana-daemon` / `kiana-protocol` | Cline、Letta、Crush、OpenCode | `not_supported`/`target` |
| J8 Observability | Trace、Receipt、Eval、cost/cache/retry metrics | `kiana-eventlog` / query | Agno、DeepSeek fixtures、OpenCode | `partial` |

**共同验收**：

- Tool Search 只发现候选，不授予 Grant；
- Memory 命中带 ACL、purpose、authority、freshness 和 provenance；
- Prompt Cache 不被误当作 Session/Event 持久化，Cache hit 按 provider/model/prompt version 分桶；
- Workflow definition 版本固定，重试、取消、Approval、补偿和恢复均可重放；
- Swarm fan-out 有 parent、partition、预算、并发、TTL、WorkFingerprint 和 MergeDecision；
- MCP server/tool schema、health、trust、version 和 result validation 可追踪；
- Provider 事件先归一化，CLI/Web/未来 SDK 不维护第二套运行循环；
- 失败、Unknown、取消、重连、压缩和持久化错误都有测试 fixture 与 Receipt assertion。

**不做**：在 P0/P1 可靠 Runtime 和事实源之前扩展百级工具、自由 `SendMessage`、无限 swarm、HTTP MCP、远程 worker 或真实外部副作用。

### Slice K：运营、治理与可靠性

**目标**：让 CompanyOS 能在长期运行中处理身份、触发、人工决策、Artifact、成本、容量、崩溃、Incident、数据治理和 Connector，而不是只完成一次 Run。

**规范来源**：[`company-os-operations-governance.md`](company-os-operations-governance.md)。

| 子切片 | canonical 合同 | 主要 owner | 参考设计 | 当前状态 |
|---|---|---|---|---|
| K1 Identity | Principal、Membership、RoleAssignment、AuthorityEpoch | `kiana-domain` / `kiana-core` | Codex Thread/Turn、OpenCode run-state、Cline host | `partial` |
| K2 Trigger | TriggerDefinition、TriggerFiring、Schedule、Signal | `kiana-daemon` / `kiana-workflow` | CrewAI Flow、Archon DAG、ChatDev Graph | `target` |
| K3 Human control | HumanTask、ApprovalInbox、Escalation、Reconciliation | `kiana-core` / `kiana-daemon` | Agno requirements、Agent Framework approval、Letta resume | `partial` |
| K4 Artifact | ArtifactVersion、WorkspaceSnapshot、PatchSet、Delivery | `kiana-domain` / `kiana-eventlog` | Roo checkpoint、Pi tree、Archon worktree、Aider diff | `partial` |
| K5 Cost/capacity | UsageRecord、CostLedger、Quota、Backpressure | `kiana-core` / `kiana-eventlog` | DeepSeek usage、Agno run usage、OpenCode processor | `target` |
| K6 Reliability | HealthStatus、Incident、RecoveryPlan、Reconciliation | `kiana-eventlog` / `kiana-daemon` | Goose recovery、Crush cancel、Cline abort | `partial` |
| K7 Data governance | DataClass、Purpose、ProcessingGrant、Retention | `kiana-domain` / `kiana-ports` | Kiana security constitution + host/tool boundaries | `target` |
| K8 Connector | ConnectorDefinition、AccountBinding、ProviderReceipt | broker / adapter crates | Goose extensions、Cline host、MCP lifecycle | `not_supported`/`target` |

**共同验收**：

- role、project、session、approval 和 scheduler 都有服务端 ownership；
- Trigger 只能创建 Workflow/Run，不能直接执行 Capability；
- Approval、Review、Acceptance、Incident 和 Reconciliation 能进入 Human Inbox；
- Artifact、Workspace、Patch、Delivery 可以追溯到 Event、Run 和 Receipt；
- RuntimeBudget、ProjectBudget 和 FinancialBudget 不混用；
- crash、timeout、cancel、disk full、MCP failure 和 Provider Unknown 都有 Incident/Recovery；
- 删除、过期和撤销能传播到 Memory、Artifact、Index、Compaction 和 cache policy；
- Connector 不绕过 ControlPlane、Approval、Idempotency、Receipt 和 reconciliation。

**当前状态**：`target`。已有 ProjectTrust、Approval、EventLog、PathLock、Receipt 和部分 Artifact/运行安全实现不能替代 durable principal、完整 HumanTask、CostLedger、Scheduler 或数据删除传播。

### Slice L：质量、学习与扩展生态

**目标**：用可重放的 Eval、版本治理和扩展审核保证质量持续提升，避免模型、Prompt、Memory、Tool、Workflow 或 Plugin 变化产生不可见回归。

**规范来源**：[`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md)。

| 子切片 | canonical 合同 | 主要 owner | 参考设计 | 当前状态 |
|---|---|---|---|---|
| L1 Eval | EvalSuite、EvalCase、GoldenTrace、QualityGate | `kiana-runner` / `kiana-eventlog` | DeepSeek fixtures、OpenCode effect tests、Cline runtime tests | `partial` |
| L2 Feedback | Feedback、Pattern、Candidate、Promotion | `kiana-query` / `kiana-domain` | Agno run feedback、MemPalace lessons、memorix | `target` |
| L3 Version governance | ModelProfile、PromptBundle、RouteDecision、DriftReport | `kiana-runner` / `kiana-protocol` | Provider-normalized designs、OpenCode processor | `target` |
| L4 Code intelligence | RepositorySnapshot、SymbolIndex、DependencyGraph、RepoMap | `kiana-query` | Aider repo map、Continue context provider、OpenHands workspace | `partial` |
| L5 Extension | ExtensionManifest、SkillPack、CapabilityPack、WorkflowPack | `kiana-skills` / broker / daemon | Cline extensions、Goose extensions、Archon packs | `partial`/`target` |
| L6 Supply chain | content hash、license、signature、capability diff、rollback | daemon / packaging | Cline/Goose host boundary、Archon artifact gates | `target` |

**共同验收**：

- Eval replay 不产生真实外部副作用；
- GoldenTrace 绑定源码快照、输入 hash、版本和 Receipt；
- 安全失败、禁止效果、replay divergence 和 evidence 缺失会阻断 Promote；
- Cache、Memory、Tool Search、Workflow、Swarm 和 Provider 指标按版本分桶；
- Feedback 只能产生候选改进，不能直接修改 Role、Grant、Policy 或历史事实；
- Code intelligence 结果带 snapshot、来源和 freshness；
- Skill/Plugin/MCP/Workflow Pack 的安装、升级、迁移、撤销和回滚可审计；
- 生态扩展不能创建第二套 Runtime 或绕过 ControlPlane。

**当前状态**：`target`。当前已有 cassette、focused regression、query/repo-map、skills 和 hooks 素材，但统一 EvalSuite、GoldenTrace、质量晋级、drift、extension supply chain 尚未形成完整闭环。

### Slice M：UI/UX 与客户端投影

**目标**：让 CLI/TTY、Web 和 Desktop 以不同呈现复用同一 State/Event/Receipt 事实，而不是各自维护会话、运行循环或权限语义。

**规范来源**：[`company-os-ui-ux.md`](company-os-ui-ux.md)。

| 子切片 | canonical 合同 | 主要 owner | 参考设计 | 当前状态 |
|---|---|---|---|---|
| M1 Workbench baseline | status line、transcript、input、trust、sandbox、cancel、receipt | `kiana-entrypoints` | Codex、Pi、Aider | `partial`（`local_behavior`） |
| M2 UI projection | `UiSnapshot`、`UiAction`、cursor、epoch、pending action | `kiana-protocol`（wire DTO）/ `kiana-daemon`（投影生成） | OpenCode、Crush、Cline | `target` |
| M3 Human actions | Approval、Review、Acceptance、Incident action card | `kiana-daemon` / `kiana-entrypoints` | Agno、Agent Framework、Letta | `target` |
| M4 Run/Artifact detail | Run timeline、Invocation、Diff、Evidence、Receipt | `kiana-daemon` / `kiana-entrypoints` | Roo Code、OpenHands、Aider | `target` |
| M5 Web sync | snapshot hydration、event subscription、reconnect、stale response guard | `kiana-entrypoints` | OpenCode、Crush、Letta | `target` |
| M6 Desktop shell | workspace onboarding、health、tray、background、safe close | `contrib/desktop` | Cline host、OpenHands UI | `partial` |
| M7 accessible fallback | keyboard、窄屏、text status、aria/high contrast | all presentation owners | Codex/TUI patterns | `target` |

**共同验收**：

- 同一个 Run 在 CLI、TTY、Web 和 Desktop 上的 terminal state 一致；
- UI action 带 target ID、owner、expected epoch/version 和 idempotency key；
- Approval、Cancel、Unknown、Failure、Reconnect 都能解释下一步；
- Receipt、Artifact、Evidence、Review 和 Acceptance 可以相互定位；
- UI 乐观更新不会覆盖更新的服务端事件；
- 颜色、动画和布局变化不会隐藏安全状态；
- Secret、隐藏 reasoning 和未授权 Memory 不进入可见或可复制内容；
- 当前未支持 token streaming、remote、live provider 和真实外部副作用不出现在“已完成”界面。

**当前状态**：`partial`/`target`。Workbench 已有 conversation/input/status、trust、sandbox、cancel 和 receipt 入口；统一 Run detail、Human Inbox、cursor hydration 和完整恢复交互尚未完成。

## 4. 能力域实施顺序

> 全局阶段序列以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 为唯一 canonical（P0–P6）；本节按能力域组织，不定义全局阶段编号。

### Coding

复用现有 shell、apply_patch、query、memory、stdio MCP 和 skills，但先修：

- shell 进程组和取消；当前 cancel 结果已区分为 `cancelled`，但仍需 effect confirmation 与 Unknown 对账；
- Builder packet path admission 使用 kernel-backed lock file 防止独立 ControlPlane 重叠接纳；Cell/Grant/Budget durable state 和 effect-time TOCTOU 仍需实现；
- path TOCTOU、symlink、hardlink 和 rename；
- cwd/env/output/时间/并发限制；
- bwrap 缺失时不得退化宿主执行；
- provider/tool args schema 和大小限制。

### Office / Work

先实现：

- 本地文档整理；
- 报告和邮件草稿；
- 任务和日程建议；
- 只读知识检索。

后实现发送、提交和外部资料修改；这些属于 R3，需要最终 payload 单次确认。

### Search / Recommendation

先实现只读搜索、来源、时间和新鲜度；搜索结果永不产生购买或预订授权。

### Commerce / Food

最后才实现购物车、下单、支付和退款。R4 必须绑定商户、商品、数量、总成本、地址、时间、条款、账户、digest 和 idempotency key；价格或条款漂移即重新确认。

### Mobility / Travel

先实现路线、报价和行程草稿；出票、打车、订房、改签和取消必须单次确认、供应商状态核验和 Unknown 对账。

### Home / IoT

先做设备读取和场景草稿。门锁、摄像、麦克风、燃气、高功率、固件、车辆和安防属于 R5，默认禁止自治，需要独立安全控制器、watchdog、急停和人工接管。

## 5. 证据与测试模板

每个实现切片必须提交以下证据（提交指进入工作记录，不代表 git commit）：

```text
source_snapshot
worktree_status
rust_toolchain
fixture_hash
input_hash
policy/profile
cassette/provider profile
exact_command
expected_event_sequence
exit_code
final_tree_hash
receipt_assertions
failure/retry/unknown semantics
```

测试分层：

1. Domain unit/property：值对象、状态机、权限交集、schema；
2. Contract：protocol、error、event、receipt、capability descriptor；
3. Core integration：fake policy/gate/broker/runner/approval；
4. Daemon integration：composition、sandbox、handler、持久化；
5. Entrypoint E2E：CLI、TTY、HTTP、Desktop subprocess；
6. Failure injection：kill-9、磁盘满、半写日志、超时、并发审批、取消、路径竞态、孤儿进程/锁。

### 5.1 切片实现卡登记位置

§1 的九项是每个切片（含 Slice J/K/L/M 的子切片）进入实现前的登记项，必须逐项填写，缺项不得进入实现。登记方式：

- 每个切片/子切片在开工前登记一张实现卡，字段模板复用 [`company-os-spec-index.md`](company-os-spec-index.md) §10 文档变更卡；owner、依赖、canonical 类型、稳定错误码、状态转移、事件序列、测试 fixture、预期 Receipt 和失败/取消/`result_unknown` 语义在卡内逐项对应。
- 本文各 Slice 只登记实现卡中的 owner、canonical 合同、参考设计、验收和当前状态；canonical 类型的主要实现位置见 §2 模块责任地图，跨文档 owner 以 spec-index §4 注册表为准。
- 测试 fixture、预期 Receipt 和失败/取消/Unknown 语义的记录格式见 §5 证据与测试模板。
- 状态与证明等级只在 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 的证据块中登记；本文表格里的「当前状态」只是索引，不构成证据，证明等级按 spec-index §6.1 的 `source` / `local_behavior` / `durable` / `live` / `physical` 成对记录。
- Slice J/K/L/M 子切片表的最小退出条件 = 该 Slice 的「共同验收」+ 实现卡中按九项填写的失败/取消/Unknown 断言；两者未同时满足不得提升状态。

## 6. Gate 0 与当前状态入口

Gate 0 必须绑定 worktree 状态、源码快照、工具链、精确命令和结果。**当前是否通过只看 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md)，不能从本文、局部 Slice 状态或历史证据块推断。**

```bash
cargo fmt --all --check
cargo check --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline --no-fail-fast
bash scripts/release-smoke.sh
bash scripts/v10-workbench-smoke.sh
```

以下三条是本文曾记录的历史证据缺口，均已由账本既有证据块关闭。保留它们只为记录曾经出现过的问题，不得据此推断当前状态：

| 历史注意事项 | 关闭它的账本证据块 |
|---|---|
| 全 workspace 测试曾返回 exit 101 | CURRENT_STATUS「Current Gate 0 revalidation after MCP, Swarm, and JSONL recovery slices (2026-09-07)」：`exit_code: 0` for every listed command |
| `kiana-core` dependency boundary 测试曾报告违规依赖 `kiana-query`、`kiana-types` | CURRENT_STATUS「G0-02 slice evidence (2026-08-28)」：`kiana-core` 已移除 `kiana-query` 与 `kiana-types`，边界测试不再报告 forbidden internal dependencies |
| Harness approval path 尚未形成 PendingInvocation round-trip | CURRENT_STATUS「P1-02 same-host continuation slice evidence (2026-08-29)」：P1-02 target → partial，PendingInvocation same-host continuation implemented |

当前仍成立的限制（以账本 limitations 为准，不得写得比账本更强）：

- `kiana-core` 仍有进程内 sessions/cancellations/path locks；显式 receipt 已用 `run.authorized` 做 owner fencing，但不构成完整跨进程恢复；
- durable authenticated principal、durable PendingInvocation/cancellation reconciliation、完整 effect-time TOCTOU 和跨进程生命周期恢复在账本 limitations 中仍为 open；
- 当前 Desktop、生活能力和外部 adapter 不能宣称生产完成。

## 6.1 Gate 0/P1 高优先级负向证据

以下负向证据来自当前代码审计和账本记录。已缓解项标注关闭它的账本证据块，残余缺口仍必须作为阻断项记录，不得被已有局部测试或对象定义覆盖：

| 证据 | 当前实现事实 | 分类 | 目标修复 |
|---|---|---|---|
| Identity | `DaemonHost` 使用固定本地主体并从 stored ProjectTrust authority 派生 project trust；wire actor/trust/profile 不能授予权限，但 role/department 仍由请求选择，尚无 durable authenticated principal | `partial` | 由受保护入口解析身份，服务端从不可变 assignment 派生 role/department/authority epoch |
| External risk | 已缓解，残余缺口 open：CURRENT_STATUS「P1-06 server-owned MCP risk boundary evidence (2026-09-02)」记录 ControlPlane 与 `DefaultPolicyEngine` 在 Broker dispatch 前拒绝 MCP risk downgrade 和 capability-kind mismatch；`capability_risk_violation` 对 `mcp.call`/`mcp` 声明 ReadOnly/LocalWrite 直接 Deny（`mcp_risk_downgrade`），非 Network capability 直接 Deny（`mcp_capability_mismatch`），ExternalSideEffect/Critical 一律 Ask。残余：namespaced operation、目标、数据和实际副作用的分级仍不完整 | `partial`（已缓解） | 按 namespaced operation、目标、数据和实际副作用重新分级；R3+ 需确认，支付/旅行/IoT 不得复用 coding grant |
| Filesystem | 已缓解，残余缺口 open：CURRENT_STATUS「P1-04 apply_patch descriptor-anchored update commit evidence (2026-09-07)」记录 Linux 单文件更新已用预打开的父目录描述符锚定临时文件创建、目标替换和模式查询，并拒绝 symlink/hardlink、独占创建临时文件；残余：add/delete/move、多 hunk 事务性、非 Linux 回退、bind-mount 替换和崩溃恢复仍未覆盖 | `partial`（已缓解） | 覆盖全部 patch 操作类型与非 Linux 路径；隔离工作树、原子多文件应用、版本 fencing 和 rollback |
| Cancellation | cancel 主要依赖 watch + Runner Cancel，并返回 Failed；没有完整 process-group/handler fence 和 effect confirmation | `gap` | 引入 authorization/dispatch/handler fences；无法确认停止时返回 `cancel_requested`/`result_unknown` |
| Ownership | continue、receipt、review 和 Web session 主要按内存 session/run 查找，未完成 authenticated owner 校验；Web 使用全局 active | `gap` | 每次请求显式 session_id，绑定 actor/project/instance，拒绝跨主体操作 |
| Event privacy | 已缓解，残余缺口 open：CURRENT_STATUS「P1-05 shell secret sentinel boundary evidence (2026-09-07)」「P1-05 centralized event/result redaction boundary evidence (2026-09-02)」记录 EventLog append、Receipt 投影、Broker→Runner 结果、直接响应和 harness 结果事件共用递归 redaction 与 server-owned 结果关联，sentinel 密钥不出现在事件、回执或后续模型请求中；残余：redaction 基于结构化 JSON 递归与键名/标记匹配，无法证明任意编码密钥检测，完整 argv/environment/stdout/stderr 扫描与 durable projector/CAS 恢复仍 open | `partial`（已缓解） | schema 化敏感字段契约、完整进程输出扫描、durable audit projector 和 append CAS |
| Approval replay | Approval decision 可以使用新的 decision request_id 作为 causation/correlation；真正风险是 pending request 的 actor/session/project/profile、request hash、目标和 approval scope 未被完整绑定，或 approval 被重复消费 | `gap` | 保留 execution request_id 作为原始执行关联；decision request_id 仅作 causation/correlation；实现 exact digest、单次 consume、expiry、nonce 和 owner 校验 |

未缓解的条款在代码修复前只能标记为 `local_behavior` 的已知限制，不得声称已强制（code-enforced）或已获得 `durable`/`live`/`physical` 证明。验收必须先覆盖 deny、越权、重放、TOCTOU、取消竞态、泄漏和恢复，再覆盖成功路径。

## 7. 90 天工程序列

| 时间 | 交付 | 退出条件 |
|---|---|---|
| 1–14 天 | Evidence Ledger、文档统一、失败分类、边界 ADR | clean/WIP 状态可区分；命令和结果可复现 |
| 15–30 天 | ID/schema/error registry、正式状态机、Web session ownership | 非法转移、重复请求、跨 session 操作有测试 |
| 31–60 天 | Cell、Template、SpawnPlan、Budget、Grant、Packet，以及 Objective/Project/Milestone/Acceptance/Outcome 合同 | 细胞可受限生成、交接 ACK、权限不可升级；目标到交付对象可关联 |
| 61–75 天 | Durable event/state/artifact、PendingInvocation、cancel fence、Company domain projector | kill-9/超时/审批/取消/Unknown 可对账；Project 状态可从事实重建 |
| 76–90 天 | Objective → Project → Planner → Builder → Reviewer → Acceptance → Closer coding 黄金闭环 | 有独立 Review、验证证据、Delivery、MergeReceipt、ClosingReceipt 和未夸大的 Outcome |

Office、Commerce、Travel 和 IoT adapter 不进入前 90 天的核心验收，只做接口设计、风险模型和模拟/只读 fixture。

## 8. 开发规则

- 不自动 commit、push、merge、release 或删除工作树；
- 不在未读目标文件前修改；
- 不把 Agent 消息、模型文本或网页内容当权限；
- 不让新 Company OS 能力进入 legacy executor；
- 不新增第二套执行循环或控制面；
- 每轮只推进一个最小可验证切片；
- 先证明 deny、unknown、replay、TOCTOU、越权、注入、泄漏和恢复，再证明 happy path。
