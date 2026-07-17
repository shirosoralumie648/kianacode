# Kiana Workflow Runtime 设计与实现协议

## 1. 目标

Workflow Runtime 把用户请求、Issue、PRD、缺陷、重构、审查、QA 或发布任务转换为可持久化、可恢复、可审计的 `WorkflowRun`。

它不是一张只用于提示词的流程图，而是 CLI、TUI、Bridge、Agent、Skill、Review、Memory、EDA 和 Release 共用的运行协议。所有入口最终都必须落到同一套对象、事件、状态、证据和门禁模型，避免每个功能形成独立且不可追溯的执行路径。

## 2. 核心对象

`WorkflowRun` 是复杂任务的持久执行单元。一个 run 至少拥有：

- `workflow_id`：跨尝试保持稳定的工作流身份。
- `run_id`：一次具体执行尝试的身份。
- `artifact_dir`：`.kiana/workflows/<run_id>/`。
- `workflow_dag.json`：本次运行采用的 DAG 模板快照；其 SHA-256 在 `WorkflowCreated` 中绑定。
- `eventlog.jsonl`：追加写入的事实源。
- `state.json`：当前节点、状态、checkpoint 和 gate 的物化投影。
- Markdown artifacts：问题定义、发现、计划、审查范围、handoff、learnings。
- JSON packets：WorkPacket、ResultPacket、VerificationPacket、ReviewPacket。

`state.json` 不是权威历史。任何状态都必须能够从完整 EventLog 重建；投影与 EventLog 冲突时，以 EventLog 为准并进入 repair 或 blocked 流程。投影校验不只比较事件序号，还必须逐项比较 `schema`、`workflow_id`、`run_id`、`status`、`current_node`、`request`、`input_kind`、`profile`、`approval_required`、`checkpoint`、`pending_approvals`、`created_at_ms` 和 `updated_at_ms`，并要求 artifact 目录名等于 `state.run_id`。`created_at_ms` 必须与 `run-<timestamp>-<suffix>` 身份一致，`updated_at_ms` 必须等于该历史前缀最后一条事件的时间。

## 3. 运行时类型

首版由 `kiana-tasks` 提供以下库对象：

- `WorkflowInit`：用户请求、输入类型、执行 profile 和审批标志。
- `WorkflowRun`：持久化运行元数据。
- `WorkflowDagTemplate`：默认 DAG 的节点与边契约。
- `WorkflowNodeSpec`：节点动作、门禁和产物定义。
- `WorkflowEdge`：带 gate decision 的允许迁移。
- `WorkflowEvent`：追加写入的运行事件。
- `WorkflowState`：用于恢复、TUI 和 Bridge 投影的状态快照。
- `WorkPacket`：带边界、验证命令和风险标志的执行包。
- `ResultPacket`：执行结果、改动、命令和偏差记录。
- `EvidenceEvent`：验证、审查、阻塞和批准事实。
- `VerificationPacket`：检查结果和 evidence 引用集合。
- `ReviewPacket`：审查 finding 和 evidence 绑定集合。
- `GateResult`：规范化门禁结果。

## 4. 默认 DAG

默认 full profile 覆盖：

```text
runtime_init
  -> capture
  -> product_definition
  -> context_intake
  -> intent_router
  -> research
  -> design
  -> plan
  -> plan_confirmation
  -> autoplan_review
  -> decision_briefs
  -> build_workpacket
  -> skill_router
  -> execute
  -> quality_gate
  -> behavior_verify
  -> review_scope
  -> multi_review
  -> review_triage
  -> ship
  -> learn
  -> loop_controller
  -> completed
```

Clarification、split goal、approval、security block、fix loop、QA planning、investigation、re-plan 和 handoff 是受控分支，不允许通过任意字符串跳转绕过 gate。

## 5. Artifact 契约

初始化时创建稳定目录形状，调用方不得猜测路径：

```text
.kiana/workflows/<run_id>/
  workflow_dag.json
  eventlog.jsonl
  state.json
  context_pack.md
  problem-definition.md
  findings.md
  task_plan.md
  plan-confirmation.md
  autoplan-review.md
  decision-briefs.md
  review/
    scope.md
  reviews/
  workpackets/
  resultpackets/
  evidence/
  verification/
  next-agent-handoff.md
  learnings.md
```

VerificationPacket 与 ReviewPacket 使用安全 artifact ID，先写同文件系统临时文件，再以不可覆盖方式发布。最终文件已存在时返回冲突；遗留 `.tmp` 文件不参与 Pass/Done 投影，并由 strict audit 作为 blocker 报告。

## 6. EventLog 与并发写入

以下行为必须写入 `eventlog.jsonl`：

- 节点进入和退出。
- gate decision。
- artifact 发布。
- retry、block、approval 和 risk acceptance。
- verification/review 完成。
- repair、fork、cancel 和最终状态变化。

同一 run 的 writer 由 `.eventlog.lock` lease 串行化。Linux 上持锁 PID 仍存在时返回 `WriterBusy`；PID 已不存在时可回收 stale lock；无法判断 PID 时必须超过有限 TTL 才能回收。

显式 event ID 或关键 data value 的唯一性检查必须和 append 位于同一 writer lease 内。禁止使用无锁的“先查询、后写入”，否则并发 worker 可以写出重复完成事件。

EventLog append 和 `state.json` 投影更新位于同一个 writer lease，但它们不是单文件事务。运行时因此明确承认一种合法 crash window：事件已经完整写入并通过认证，进程在更新 state 前退出。恢复器只能修复这一种“state 落后”状态，不能把任意不一致解释为崩溃。

`state.json` 使用 durable atomic replace：在目标同目录创建唯一临时文件，写入完整 JSON，执行 `flush` 和 `sync_all`，再原子替换目标。Unix 使用同文件系统 `rename`，随后同步父目录；Windows 使用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`。失败时清理临时文件；遗留 `.state.json.tmp-*` 不得成为运行投影或完成证据。

`WorkflowCreated` 必须记录 `dag_sha256` 和 `initial_node`。每次 runtime transition 在获取 writer lease 后重新读取持久化 DAG，验证 schema、节点唯一性、edge 端点和摘要，再确认 `(from, decision, to)` 是 DAG 中真实存在的 edge。命令层路由不能替代该校验，底层 API 同样不得绕过 DAG 边界。遗留 run 没有 `dag_sha256` 时，只允许其 DAG 与当前默认模板结构完全一致；任意自定义或被改写的快照必须 fail closed。

## 7. Verification 与 Review 一致性

一个 VerificationPacket 只有在 EventLog 存在字段完全匹配的 `VerificationCompleted` 时才有效。必须同时匹配：

- `verification_id`。
- `workflow_id/run_id`。
- `final_status`。
- `packet_path`。
- required checks 与 evidence refs。
- Task metadata 中的 verification 引用。

孤儿 packet、伪造 pass、跨 run 引用、路径穿越和 packet/事件字段不一致都必须阻塞 Board、report 和 audit。

ReviewPacket 的每个 finding 必须恰好绑定一个当前 WorkflowRun 内真实存在的 evidence ref。ReviewPacket 和 `ReviewCompleted` 必须记录相同的 workflow/run 和 packet path。EventLog 损坏时可以返回内存中的 blocked 报告，但不得声称 packet 已持久化。

`audit strict` 和 `report progress` 的运行发现分为两层：正常路径必须使用完整 EventLog 投影校验后的可信 run 列表；可信列表失败时，报告入口可以只读扫描 `.kiana/workflows/*/state.json`，并以目录名作为 `run_id` 定位待诊断对象。该降级路径只用于生成 blocker，不写回 state、不发布 ReviewPacket，也不把 state 视为可信完成证据。后续 integrity、resume 和 evidence 读取仍按严格协议执行，因此损坏账本只能得到 blocked 报告。

活动 blocker 使用 `supersedes_event_id` 链投影。解除 blocker 时追加 superseding event，不删除历史；再次 strict audit 时复用仍未解除的 blocker event，避免重复制造等价记录。

## 8. 恢复协议

### 8.1 恢复顺序

1. 解析显式 `run_id`；无参数 `continue` 只读选择 latest candidate，不先调用要求 state/EventLog 完全一致的 run list。
2. 获取目标 run 的 writer lease，避免恢复过程中并发 append 或二次修复。
3. 读取并校验完整 EventLog 的 JSON、连续序列、事件身份、HMAC 链、引用和 artifact 绑定。
4. 读取 `state.json`，记录恢复前 `last_event_seq`。
5. 若 state 与 EventLog 尾部序号相同，要求完整状态投影逐字段一致；一致返回 `ready`，否则返回 `blocked`。
6. 若 state 落后，只使用 `state.last_event_seq` 对应的历史前缀重建投影，并要求旧 state 与该前缀逐字段一致。
7. 前缀证明通过后，从完整 EventLog 重建 state，使用 durable atomic replace 写回，再次执行完整尾部投影校验；成功返回 `repaired`。
8. 若 state 超前、目录身份冲突、EventLog 损坏、前缀字段被篡改或写回后复验失败，返回 `blocked`，不得猜测或覆盖。

### 8.2 自动修复前提

自动修复必须同时满足：

- `state.last_event_seq < eventlog.last_seq`。
- 完整 EventLog 已通过结构、序列和认证链验证。
- 旧 state 的 `schema`、目录身份、workflow/run 身份、请求、输入类型、profile、审批配置、状态、当前节点、checkpoint、pending approvals 和时间戳与历史前缀完全一致。
- 目标 state 写回发生在 writer lease 内，并通过写回后的完整投影复验。

任何一个条件不满足都必须 fail closed。尤其不能用完整 EventLog 直接覆盖一个身份、请求、节点、审批或时间戳已被修改的旧 state，因为这无法区分正常 crash 与主动篡改。

### 8.3 恢复结果三态

- `ready`：state 与 EventLog 尾部完全一致，没有发生写回。
- `repaired`：旧 state 被历史前缀证明为可信且仅落后，已从完整 EventLog 原子重建并复验。
- `blocked`：无法证明自动修复安全；`blocker` 给出不一致字段，`recommended_action=repair:reconcile-eventlog`。

命令层统一使用 `WorkflowResumeStatus::can_continue()`：`ready | repaired` 可以继续执行 workflow advance、swarm、validate 和 evidence verify；只有 `blocked` 拒绝。JSON 和文本输出必须保留 `repaired`，不能把自动修复伪装成普通 ready。

### 8.4 诊断与审计边界

`report progress` 在 EventLog 损坏时可以只读使用 `state.json` 定位 run 并报告 blocker，但不能写回 state、发布 ReviewPacket 或声明恢复成功。`workflow continue`、`validate`、`evidence verify`、integrity 和 release proof 始终走严格恢复协议。

不允许跳过 EventLog 中间坏行、忽略重复完成事件、用 state 覆盖 EventLog、在 dirty ownership 不明确时继续覆盖工作区，或把外部恢复/发布证据缺失解释为本地成功。

## 9. Gate 结果

所有 gate 只能返回规范化结果：

- `continue`
- `proceed_with_caution`
- `ask_user`
- `replan`
- `reroute`
- `fix`
- `accept_risk`
- `blocked`
- `cancelled`
- `completed`

高风险、破坏性、外部发布、凭据暴露、付款、merge、deploy 和删除操作必须经过显式 approval，不允许由默认值静默通过。

## 10. 真实 CLI 入口

WorkflowRun 当前挂载在 `tasks workflow`：

```bash
kiana tasks workflow template --json
kiana tasks workflow init --json --type feature --profile standard "request"
kiana tasks workflow list --json
kiana tasks workflow show <run_id>
kiana tasks workflow continue --json <run_id>
kiana tasks workflow advance --decision <decision> --evidence <workflow-relative-path>... [--note <text>] [--json] <run_id>
kiana tasks workflow complete --verification <verification_id> [--json] <run_id>
```

`advance` 的目标节点只能来自当前持久化 DAG 的唯一 edge。命令层负责决策解析与证据路径校验，runtime 层负责验证 `dag_sha256`、DAG 结构和真实 edge，并在一个 writer lease 内连续构造、签名和追加 `gate_evaluated`、`node_exited`、`node_entered` 三个记录。每个记录按前一个完整 record 的 SHA-256 继续 HMAC 链；三条 JSONL 在同一 buffer 中追加、flush、`sync_data`，最后只用 `node_entered` 更新 state 投影。这样 crash window 至多表现为“EventLog 已持久化但 state 尚未投影”，现有 resume reconciliation 会阻塞并要求恢复，不会出现 state 领先事实源。

`complete` 不负责生成验证结论。它只消费由 `kiana validate` 或等价受信入口写入的不可覆盖 VerificationPacket，重新执行 packet integrity 与 completion validation，并要求 packet 与 run 的 workflow/run ID 一致且 `final_status=pass`。`workflow_completed` 绑定 verification ID、packet path、SHA-256 和检查计数；同 packet 重试幂等，不同 packet、foreign packet、tampered packet、非 pass packet全部 fail closed。

证据、审计和报告入口：

```bash
kiana evidence verify --json --workflow <run_id>
kiana audit strict --json --workflow <run_id>
kiana report progress --json --workflow <run_id>
```

## 11. 当前实现切片

当前本地实现已经覆盖：

1. 默认 DAG 模板与 WorkflowRun 初始化。
2. artifact 目录、EventLog 和 state 投影。
3. workflow list/continue、`ready | repaired | blocked` 三态与冲突阻塞。
4. Project Board 对 VerificationPacket 的严格投影。
5. Evidence verify、strict audit 和 progress report。
6. VerificationPacket、ReviewPacket 不可覆盖发布。
7. writer lease、stale lock 恢复和持锁唯一性写入。
8. unresolved blocker supersession 投影。
9. evidence-bound DAG `advance`，包含三事件连续签名批写与最终 state 投影。
10. VerificationPacket-gated `complete`，包含同 run 复验、packet SHA-256 和幂等完成。
11. EventLog 驱动的完整 state 投影校验，覆盖 schema、目录/运行身份、请求、配置、节点、状态、checkpoint、pending approvals 和时间戳。
12. `dag_sha256` 快照绑定、runtime edge 校验和遗留默认模板兼容边界。
13. audit/report 的只读降级发现，可在 ledger 损坏时返回 blocker 而不伪造可信状态或持久化结论。
14. EventLog 已落盘而 state 落后的 prefix-proof 自动修复，包含 writer lease、原子写回、写后复验和命令层 repaired 继续语义。

后续节点适配器继续接入并行调度、权限审批、插件/MCP、EDA、浏览器 QA、PR、部署和外部发布证据，但不得建立第二套运行时状态模型。
# EDA Review Extension

`kiana eda review` 复用 WorkflowRun、EventLog、不可变 Artifact 和 Evidence Ledger，不建立第二套硬件状态机。命令在任何状态写入前完成项目根目录 containment 校验；新任务创建 `WorkflowInputKind::Eda + WorkflowProfile::Gated`，已有任务只允许恢复同类型且 `ready | repaired` 的 run。

`review_id` 由规则版本、输入相对路径和内容 SHA-256 确定。同一 run、同一输入的重试复用同一个 `artifact_written` 事件和 `EvidenceKind::EdaCheck`，不同内容产生新 review。输出固定为 `eda_review.json`、`bom_risk.md`、`bringup-plan.md`，并在报告中明确声明不执行 ERC/CAM、设计修改、器件替换、下单或工程签字。
