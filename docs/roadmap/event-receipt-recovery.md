# Roadmap 专项：Event / Receipt / Recovery 专项

> 返回 [Kiana 执行路线图](../roadmap.md) 的总图与当前窗口。本文保留原专项编号、状态、依赖、验收口径和证据限制；专项步骤完成不会自动改变 P 阶段状态。

## 23. Event / Receipt / Recovery 专项：事实、收据与恢复的实际设计（2026-09-12 追加）

> 本节对应 [module-map.md](../module-map.md) 的「8. Event / Receipt / Recovery：记录与恢复」。它是给实施 agent 的设计合同和步骤清单，不是完成声明。所有 `ER-*` 初始状态均为 `⏳`；当前工作树中的 journal、projection、receipt、recovery 代码已经存在的部分仍须按本节验收，不能由类型、历史测试或参考项目替代真实证据。
>
> 调研快照：`db77c2485bcafecbb1da17ec57ee509ad2ee32b4` 加 2026-09-12 变化中的 WIP。`reference/` 一级目录 73 个，目录索引约 191,308 个文件，关键词筛查命中约 13,673 个文件；这是覆盖盘点和定向源码阅读，不是逐行安全审计。外部资料只用于机制对照，不能证明 Kiana 的实现。旧 roadmap 限制与用户本次要求冲突时，以本节设计为准；已有 ControlPlane/Harness/Capability/CompanyOS 专项的合同继续复用。

### 23.1 目标、边界与当前缺口

EventLog 是唯一事实源；Receipt 是从事实、受控 Artifact 和验证结果生成的只读投影；Recovery 是在事实不完整或副作用不确定时的显式控制流程。Transcript、UI timeline、内存 map、缓存、模型文本和单独的 approval 文件都不能成为第二事实源。

当前 WIP 已提供若干可复用基础：`TransitionBatch`/`CommandReceipt`/`CommitOutcome`、JSONL v2 transition frame、目录和文件身份检查、事件递归脱敏、Run 状态和 Invocation 折叠、RunSnapshot、workspace checkpoint、approval journal、结果投递 claim，以及 `ResultUnknown`/Incident 的部分领域对象。尚未证明的关键交界包括：

| 缺口 | 需要达成的行为 | 不能用什么代替 |
|---|---|---|
| 事实提交 | 相关 authority、approval、budget、lease、permit、aggregate 一次 CAS 提交，失败返回可确认的 `Unknown` | 连续多次 `append`、内存 map 或“先写 approval 再写 event” |
| Effect 账本 | 每个 Invocation 有稳定 action digest、attempt、executor、effect/stop 证据和单一终态 | Broker 返回 `Ok`、模型说“成功”、UI 已收到输出 |
| Receipt | 可在新进程从 EventLog + Artifact refs 重算，带 owner、scope、versions、cost、files、unknown 和 limitations | 缓存的 JSON、当前请求上下文或 transcript |
| Recovery | 重启先验证 journal、重建投影、fence 旧资源、默认暂停，再由显式命令继续 | 进程重启后直接调用 runner、自动重试未知 effect |
| 对账 | 外部 effect 以 provider receipt、查询或人工证据收敛；收敛只追加新事实 | 把 `Unknown` 改写为 `Succeeded`，或删除旧事件 |

### 23.2 研究结论与吸收取舍

| 来源 | 观察到的机制 | Kiana 吸收方式 |
|---|---|---|
| [kiana-eventlog/JOURNAL.md](../../kiana-eventlog/JOURNAL.md)、`kiana-eventlog/src/journal_core.rs` | transition frame、read-set CAS、命令去重、完整页边界、尾部恢复和显式能力协商 | 作为本地事实存储基线；把“原子 authority 变更”和“外部 effect exactly-once”严格分开 |
| [Codex rollout recorder](../../reference/codex/codex-rs/rollout/src/recorder.rs) | 后台 writer 用 `Persist`/`Flush`/`Shutdown` ack 表达写入完成，终止失败可观察 | EventStore 的异步写入必须有 flush/close ack；丢失 ack 进入 `Unknown`，不能静默成功 |
| [Grok workflow journal](../../reference/grok-build/crates/codegen/xai-workflow/src/journal.rs) | 有界 JSONL、dense sequence、request hash、replay divergence、journal full fail-closed | 为模型／workflow 非确定操作保存输入摘要和结果；请求不匹配时停止，不把新脚本当旧历史继续跑 |
| [Beads event record](../../reference/beads/internal/eventsjournal/record.go) | 一个稳定的公开 envelope，存储行与发布行分离，序号在 mutation transaction 内生成 | EventLog 内部 frame 与对外事件 DTO 分离；CLI、Web、导出复用同一个投影，不各自拼字段 |
| [OpenCode SyncEvent](../../reference/opencode/packages/opencode/src/sync/README.md) | aggregate、sequence、schema；projector 先于可同步 mutation；旧 Bus 只做兼容投影 | 新事件定义必须有 schema/version/aggregate；兼容事件不得绕过 projector 或直接改库 |
| [LangGraph persistence](https://docs.langchain.com/oss/python/langgraph/persistence) 与本地 `reference/langgraph/libs/checkpoint` | thread checkpoint 与跨 thread store 分离；super-step 边界 checkpoint；pending writes 避免重跑已成功节点 | RunSnapshot 只保存可序列化且已提交状态；Invocation pending writes 单独落账；checkpoint 与长期 Memory 不混用 |
| [Temporal replay](https://github.com/temporalio/documentation/blob/main/docs/encyclopedia/workflow/workflow-execution/workflow-execution.mdx) | replay 依赖历史；workflow command 必须与历史一致；外部 Activity 与 workflow 逻辑分离 | Recovery 重放只折叠事实，不重新请求模型、执行 shell 或 MCP；不把 Kiana 变成完整 Temporal 服务 |
| [Restate durable steps](https://docs.restate.dev/develop/ts/durable-steps) 与 [DBOS steps](https://docs.dbos.dev/python/tutorials/step-tutorial) | 非确定 I/O 包在 durable step 中；结果可重放；retry 按次数／时间／错误类型约束 | 每次 provider/tool/effect 形成独立 Invocation attempt；retry policy 持久化，默认一次，未知 effect 禁止自动 retry |
| [OpenHands Agent](https://docs.openhands.dev/sdk/arch/agent)、Pi recovery、Cline checkpoint restore | 单 step 推进、pending action、部分输出恢复、工作区 restore transaction | Harness 只推进一个事实驱动 step；部分 provider 输出进入 Artifact/Unknown；workspace restore 先捕获再 commit/rollback |
| 本地 `adk-python` replay、`agent-framework` checkpoint、`pydantic-ai` durable exec、Goose/Crush/Letta Code 的 session/approval persistence | replay barrier、checkpoint metadata、operation identity、approval recovery 与慢消费者分离 | 每个 operation 具有稳定 identity 和 parent/attempt；审批恢复重新核验 scope；展示消费者不能阻塞事实提交 |
| [OpenTelemetry signals](https://opentelemetry.io/docs/concepts/signals/) | trace、metric、log 是不同信号，日志可关联 TraceId/SpanId | `correlation_id`/`causation_id` 进入事件和 Receipt；telemetry 只观察事实，不产生授权或状态 |

参考项目常见的直接工具执行、宽松 session fallback、自动 retry、宿主权限 shell、把内存 checkpoint 当 durable、或把 UI event 当事实，都不直接采用。

### 23.3 目标代码设计

#### 23.3.1 三层存储和权威关系

```text
EventStore (append-only facts, CAS, command dedup, cursor)
       |
       +--> Projectors (Run / Invocation / Approval / Resource / Company)
       |        |
       |        +--> ReceiptQuery (read-only, rebuildable)
       |        +--> RecoveryIndex (pending / unknown / incidents)
       |
       +--> ArtifactStore (immutable bytes, hash, redaction, retention)
```

- **EventStore** 只接受已经由 ControlPlane 构造并授权的 `TransitionBatch`。它验证 schema、command digest、read set、aggregate stream version、contiguous writes、frame checksum、容量和同步结果；它不推导权限、不创建 permit、不把外部副作用标成成功。
- **Projector** 是纯折叠或带明确 checkpoint 的可重建读模型。投影失败保留原事件、报告健康故障并允许从指定 cursor 重放；不得修正或删除事实。
- **ArtifactStore** 保存大输出、patch、文件 changeset、provider receipt 和恢复证据的不可变版本。事件只保存 hash、大小、分类、provenance 和受控 ref；Receipt 不能因为读取 artifact 失败而伪造空值。
- **缓存** 只能加速投影。缓存 miss、epoch 不符、checksum 不符或 schema 过期必须回 EventStore；回读失败是错误或 `Unknown`，不是空集合。

#### 23.3.2 事件 envelope 与分类

沿用 `RuntimeEvent` 的 `event_id`、`request_id`、`sequence`、`kind`、`data`、`aggregate_type`、`aggregate_id`、`stream_version`、`idempotency_key`。在不破坏旧读取的前提下，为 v2 payload/contract 补齐以下字段：

```text
EventEnvelope {
  schema, schema_version, event_id, command_id, request_id,
  correlation_id, causation_id, parent_event_id?,
  actor_ref, project_ref, session_id?, run_id?, turn_id?,
  invocation_id?, execution_id?, attempt?,
  aggregate_type, aggregate_id, stream_version, logical_cursor,
  occurred_at, observed_at, redaction_profile, payload, payload_digest
}
```

`logical_cursor` 由 EventStore 在完整 commit 后分配；同一 transaction 的事件共享 commit boundary，分页不得拆分。`occurred_at` 是来源时间，`observed_at` 是写入观察时间；时间仅用于诊断和策略输入，不能替代 stream version。`actor_ref` 必须来自服务端主体，不能从模型或 UI 字段采信。

稳定 kind 按事实归类：

| 类别 | 事件示例 | 必须记录 |
|---|---|---|
| command/authority | `request.accepted`、`run.authorized`、`action.authority_pinned` | immutable command digest、principal、authority/data epoch、scope |
| lifecycle | `run.queued`、`run.started`、`run.cancelling`、`run.completed`、`run.failed`、`run.cancelled`、`run.result_unknown` | run/turn、状态原因、terminal 约束、关联 incident |
| model | `run.prompt`、`run.model_turn`、`run.delta`、`run.compact` | prompt/model 输入摘要、完整响应 artifact ref、usage、stop reason；敏感原文按治理策略处理 |
| invocation/effect | `run.capability_requested`、`execution.prepared`、`invocation.dispatching`、`execution.result_committed`、`capability.completed/failed/result_unknown` | invocation/execution/attempt、action digest、executor/backend、effect_known、stop_confirmed、result/artifact refs |
| approval/resource | `approval.requested/approved/denied/expired/consumed`、`budget.reserved/settled`、`lease.fenced/released` | exact subject、nonce、expiry、read versions、实际用量、fencing token |
| artifact/workspace | `artifact.created/verified/revoked`、`workspace.checkpoint/restore_*`、`changeset.committed` | content hash、workspace identity/revision、path set、verification result |
| incident/recovery | `incident.opened`、`recovery.proposed/approved/executing/verified/failed/abandoned`、`reconciliation.requested/observed` | observed state、last durable cursor、safe/forbidden action、证据 ref、审批和结果 |

事件 payload 必须 `deny_unknown_fields` 或有明确的 schema migration。未知的 **required** kind、字段或版本在恢复和写入边界 fail-closed；仅标记为 `optional/forward-compatible` 的字段可以由旧读者忽略，并在 Receipt 的 `limitations` 中可见。

#### 23.3.3 三类 Receipt，不混淆证明含义

1. **CommandReceipt**：说明一个 `command_id + command_digest` 已在 EventStore 中被接受一次，包含 commit id、first/last cursor、event ids、aggregate final versions。它证明事实提交，不证明 handler 已运行或外部 effect 已发生。
2. **ExecutionReceipt**：由 `execution.prepared`、`invocation.dispatching`、`execution.result_committed` 和 stop/effect 证据折叠而来。字段至少包括 action digest、attempts、executor/backend、started、effect_known、stop_confirmed、provider receipt/artifact refs、resource settlement 和 unknown reason。只要有一个可能发生副作用且未确认的 attempt，Invocation 终态就是 `Unknown`。
3. **RunReceipt**：从 run/turn/model/invocation/artifact/workspace/approval 事实投影，包含 owner、scope、status、terminal reason、model turns、cost ledger、files_changed、artifact refs、invocations、memory hits、approval history、incidents、data epoch、authority revision、last durable cursor 和 limitations。RunReceipt 的 `Completed` 只表示本机事实链已完成，不表示业务 Outcome 已达成。

业务 `DeliveryReceipt`、`ClosingReceipt` 和 `Outcome` 继续由 CompanyOS 产生，并引用 RunReceipt/EvidenceBundle；不得把它们塞进运行时 receipt 或以模型文本代替验收。

所有 Receipt 必须带 `schema`、`projection_version`、`source_cursor`、`source_event_ids`、`generated_at`、`redaction_profile`、`owner`、`feature_status` 和 `proof_level`。同一 cursor、同一 projection version、同一 artifact hash 应产生稳定内容；由于 `generated_at` 变化而造成的差异只能存在于非事实 metadata。

#### 23.3.4 事务和 effect 边界

一次控制面变更采用如下边界：

```text
read authority / approval / budget / lease / aggregate versions
  -> validate and compute immutable action digest
  -> commit TransitionBatch (all facts and resource reservation)
  -> only after Committed/Replayed dispatch the prepared permit
  -> broker executes one attempt under fence
  -> commit result/effect/usage/lease settlement atomically
  -> deliver committed result to Harness exactly once per result-delivery command
```

`CommitOutcome::Conflict` 必须重新读取并重新授权；绝不能只替换 expected version 重放旧决定。`Unknown` 必须调用 `read_command(command_id)` 确认原提交；确认前不得 dispatch、retry、补写一套新 command 或释放仍可能被使用的资源。EventStore 不能保证外部 effect exactly-once，因此 Broker/connector 要求 idempotency key、provider receipt 或对账接口；没有这些条件时，超时和进程崩溃都进入 `Unknown`。

#### 23.3.5 恢复对象和状态机

```text
RecoveryCase {
  recovery_id, incident_id, target_ref, target_kind,
  observed_state, last_durable_cursor, source_event_ids,
  safe_actions[], forbidden_actions[], requires_human,
  authority_revision, data_epoch, status, evidence_refs[]
}

Incident: Open -> Classified -> Fenced -> Reconciling -> Resolved | Abandoned
RecoveryPlan: Proposed -> Approved -> Executing -> Verified | Failed
                         \-> Abandoned
```

重启后的默认状态是 `Paused/NeedsRecovery`：EventLog 健康、投影重建、pending/unknown 列表可查询，但没有新的 `run`、`execution` 或外部 effect 被自动启动。只有显式 `resume`、`retry_without_effect`、`reconcile`、`restore_workspace` 等命令，重新通过当前 policy/gate/approval 后才可继续。恢复命令本身也是一个带 command digest 的 transition。

### 23.4 端到端处理流程

#### 23.4.1 正常 Run 与工具调用

```text
Entry point receives request
  -> DaemonHost derives authenticated context and project identity
  -> ControlPlane validates schema, trust, role, budget, path and epoch
  -> commit run.authorized / run.queued (CommandReceipt)
  -> Harness loads ContextPlan and records run.prompt
  -> Provider returns one normalized model turn
  -> commit run.model_turn (full response in bounded Artifact if needed)
  -> for each tool call: record run.capability_requested with stable call_id
  -> ControlPlane runs hook/policy/gate/approval and computes action digest
  -> commit approval consumption + budget reservation + execution.prepared
  -> Broker verifies permit, scope, epoch and fencing; records invocation.dispatching
  -> handler runs inside selected backend and returns bounded output/stop report
  -> commit execution.result_committed + usage + lease settlement
  -> deliver the committed result to Harness once; record capability.completed/failed
  -> Harness either starts the next turn or emits one terminal run event
  -> RunReceipt projection reads the ledger and artifacts
```

The model is never allowed to turn a receipt, event payload, tool description, or web content into authority. A tool failure that is known and has no uncertain effect is fed back as a structured tool result; a persistence or effect uncertainty stops the run.

#### 23.4.2 Approval pause and resume

1. Persist `approval.requested` with exact request hash, action digest, scope, nonce, expiry, authority/data versions and original invocation identity. The pending item is visible through Human Inbox and `list_pending_approvals`.
2. On approve/deny, validate the authenticated approver, CAS the approval stream, and consume it once. A changed argument, path, schema, epoch, role, or project identity produces `approval_action_changed`/`approval_authority_changed` and no dispatch.
3. Before daemon shutdown, checkpoint the serializable Harness state and pending invocation. Redaction that changes the input sets `resumable=false`; an unavailable or malformed checkpoint leaves the run paused/unknown.
4. A new process loads the snapshot only after owner, role prompt hash, sandbox, authority revision, data epoch, path scope and approval proof match. It appends `run.resume_prepared` with a CAS against the exact snapshot/run version, restores the runner, then re-runs the same dispatch preflight. It does not trust a UI-supplied run context.

#### 23.4.3 Cancel and shutdown

`cancel_requested` is an intent, not proof of stop. ControlPlane appends a cancellation generation, marks pending work not-started as `not_executed`, asks every started handler/process group to stop, waits for a bounded `StopReport`, and records `stop_confirmed` per execution. If any started effect or process cannot be confirmed, the run and affected Invocation remain `ResultUnknown`/`Unknown`, resources stay fenced, and a RecoveryCase is opened. A late result is accepted only if its execution fence and result-delivery claim still match; it cannot resurrect a terminal run.

#### 23.4.4 Restart and boot recovery

```text
open journal -> validate header/frames/checksums/torn tail -> sync complete prefix
  -> verify schema migrations and store capabilities
  -> fold projections from last trusted checkpoint + remaining cursor
  -> rebuild pending approvals, leases, reservations, invocations, incidents
  -> fence stale workers and refuse ambiguous ownership
  -> mark orphaned dispatches Unknown; create incidents/recovery plans
  -> expose read-only status and Recovery Inbox
  -> wait for explicit, newly authorized recovery command
```

The boot path must distinguish an empty store, unsupported read, corrupt store, and a valid store with no matching events. A malformed first frame, checksum mismatch, duplicate header, gap, conflicting terminal, or unknown required schema is a startup failure or quarantine, never an empty history.

#### 23.4.5 Unknown 对账与重试

| 观察事实 | 允许动作 | 禁止动作 |
|---|---|---|
| prepare 未提交且无 dispatch | 重新授权后创建新 execution | 使用旧 permit 或假定 effect 已发生 |
| dispatching 已提交，handler 未确认开始 | 先 fence/查询进程和 backend；无 effect 证据才可新 attempt | 直接 retry 原 operation |
| handler 已开始、结果/stop 未确认 | `Unknown` + Incident + Reconciliation | 自动 retry、释放写锁、发布成功 |
| effect receipt 明确成功 | 以原 receipt 完成 reconciliation；不重复 effect | 再执行一次只为“补回调” |
| effect receipt 明确失败且无副作用 | 按持久 RetryPolicy 新 attempt，新的 execution id 和 idempotency key | 修改旧 attempt 事件 |
| provider/connector 不支持查询 | 永久 `Unknown` 或人工证据；资源保持 quarantine 直到决定 | 把 timeout 映射成 failed/no-effect |

重试永远生成新的 `attempt` 和事件；Invocation identity/action digest 保持可关联，execution/permit/approval 按需要重新签发。`maximum_attempts=1` 是默认值；PolicyDenied、SecurityViolation、scope/epoch mismatch、unknown effect 和 terminal run 永不自动 retry。

### 23.5 详细实施步骤（ER-00–ER-36）

依赖顺序是 `ER-00 → ER-01..06 → ER-07..12 → ER-13..19 → ER-20..27 → ER-28..33 → ER-34..36`。每张卡都按“先拒绝、再成功、最后回归”执行；实现者可在卡内拆 commit，但不得跳过负向证据。`CP-*`、`H-*`、`CAP-*`、`CO-*` 是交叉引用，不新增第二套执行循环。

#### A. 事实契约和 journal

<a id="step-er-00"></a>



##### ER-00 — 固定基线与事实边界　⏳

- **落点：** `docs/module-map.md`、`CURRENT_STATUS.md`、`kiana-eventlog`、`kiana-core`、`kiana-domain`；关联 `CP-00`、`H01`、`CAP-00`。
- **动作：** 记录精确 HEAD、相关 WIP 文件 hash、EventStore capabilities、已有事件 kind 和当前失败；画出 command/event/effect/receipt 四个边界。标记哪些 map 是缓存、哪些文件是事实、哪些现有测试只证明 source/local behavior。
- **先拒绝：** `recovery_baseline_does_not_treat_cache_as_fact`、`empty_store_is_not_read_failure`。
- **成功/证据：** 一个最小 run/tool/approval/unknown fixture 能列出完整 ID 链和事实来源；无代码状态提升。

<a id="step-er-01"></a>



##### ER-01 — 事件 schema、kind registry 与迁移规则　⏳

- **落点：** `kiana-domain/src/contracts.rs`、`states.rs`、`journal.rs`、`kiana-protocol`；关联 `P0-A-01b/02`、`CP-02/03/28`。
- **动作：** 建立 machine-readable `EventKindSpec`（schema/version、aggregate、required IDs、terminal/secret policy、migration）；为现有 RuntimeEvent 保留 legacy decode，新增 required/optional 字段规则和 unknown kind policy。
- **先拒绝：** `unknown_required_event_kind_fails_closed`、`event_schema_version_cannot_downgrade`、`event_payload_unknown_field_is_not_silently_dropped`。
- **成功/回归：** 每个公开 kind 有 owner、serde round trip、migration fixture 和一条投影转换测试；旧 JSONL 只读兼容继续通过。

<a id="step-er-02"></a>



##### ER-02 — 统一身份、关联和顺序语义　⏳

- **落点：** `kiana-domain` ID contracts、`kiana-core/events.rs`；关联 `P0-G-02a/02b`、`H02/H10/H13`。
- **动作：** 明确 `command_id`、`request_id`、`session_id`、`run_id`、`turn_id`、`invocation_id`、`execution_id`、`attempt`、`event_id` 的 owner 和生命周期；补 correlation/causation/parent 关系，禁止以 request-local sequence 代替 aggregate version。
- **先拒绝：** `event_id_reuse_is_denied`、`same_request_different_command_digest_conflicts`、`cross_run_result_cannot_pair_by_sequence`。
- **成功/回归：** direct、Harness、approval-resume 三入口产生同形 identity chain；legacy 无 stream metadata 仅走明确兼容路径。

<a id="step-er-03"></a>



##### ER-03 — 事件边界脱敏和 Artifact 引用　⏳

- **落点：** `kiana-core/events.rs`、`redaction.rs`、`kiana-domain/redaction.rs`、Artifact ports；关联 `CP-25/26`、`CAP-13/18/19`。
- **动作：** 在唯一 EventStore boundary 做递归脱敏、payload size/depth limit、secret scan 和 protected artifact ref；记录 redaction profile、原文是否可恢复和数据 epoch。
- **先拒绝：** `secret_never_enters_event_or_receipt`、`redaction_changes_snapshot_marks_non_resumable`、`oversize_payload_is_rejected`。
- **成功/回归：** 普通字符串、嵌套 JSON、无效 UTF-8、跨 chunk secret、artifact hash 和 legacy event 都有可核对投影；不把脱敏失败当空输出。

<a id="step-er-04"></a>



##### ER-04 — CommandReceipt 与 transition read-set　⏳

- **落点：** `kiana-domain/src/journal.rs`、`kiana-ports/src/lib.rs`、`kiana-core/dispatch.rs`；关联 `CP-06/13/14`。
- **动作：** 固化 `TransitionBatch` 的 command digest、所有 authority/resource/aggregate read versions、contiguous stream writes、CommitOutcome 语义；把 `read_command` 作为 Unknown 的唯一确认入口。
- **先拒绝：** `transition_missing_dependency_is_denied`、`read_set_conflict_appends_nothing`、`unknown_commit_never_dispatches`。
- **成功/回归：** 同 command+digest 返回同 receipt；同 command+不同 digest 返回 conflict；重试不追加第二组事件。

<a id="step-er-05"></a>



##### ER-05 — JSONL v2 原子 frame、锁与损坏策略　⏳

- **落点：** `kiana-eventlog/src/jsonl.rs`、`journal_core.rs`、`JOURNAL.md`；关联 `P0-G-04`、`CAP-24`。
- **动作：** 完成 header/frame upgrade、flock、dirfd/no-follow、write/flush/sync/identity 检查、logical cursor、whole-transaction page；区分可修复 torn tail 与完整 malformed/checksum failure。
- **先拒绝：** `malformed_first_frame_is_not_empty_store`、`legacy_writer_after_v2_header_is_denied`、`write_or_sync_failure_returns_unknown`、`cursor_cannot_split_commit`。
- **成功/回归：** reopen、same command replay、disk full、file replacement、symlink parent、partial final line、concurrent writers 和 capacity limit 全部有 fixture；Memory adapter 明确不声称 durable。

<a id="step-er-06"></a>



##### ER-06 — 异步写入、背压与 shutdown ack　⏳

- **落点：** `kiana-eventlog` async adapter、`kiana-daemon` lifecycle；关联 `CP-27`、`H33`。
- **动作：** blocking file I/O 使用 bounded `spawn_blocking`/worker queue；提供 `flush`, `close`, `health`, `last_durable_cursor`；队列满、worker panic、shutdown 超时都返回结构化错误。
- **先拒绝：** `eventlog_worker_queue_full_is_bounded`、`flush_timeout_is_not_success`、`writer_panic_is_observable`。
- **成功/回归：** 多 producer、慢磁盘、取消 async task、daemon shutdown/reopen 后 cursor 和 receipt 一致；没有无限等待或丢 terminal event。

#### B. 投影与 Receipt

<a id="step-er-07"></a>



##### ER-07 — 通用 replay reader 和 projector checkpoint　⏳

- **落点：** `kiana-core/projection.rs`、新建或复用 query projection store；关联 `P0-G-04`、`P2-J5-01`、`CAP-25`。
- **动作：** 定义 `fold(cursor, events)`、checkpoint digest、schema compatibility、rebuild-from-zero；投影 checkpoint 只优化读取，必须能被原始事件替换。
- **先拒绝：** `projector_failure_keeps_source_events`、`checkpoint_digest_mismatch_forces_rebuild`、`projection_cannot_issue_authorization`。
- **成功/回归：** 从零折叠与 checkpoint+tail 折叠 byte-equivalent；重复 event id 幂等；投影进程重启可继续。

<a id="step-er-08"></a>



##### ER-08 — Run/Turn 状态投影和 terminal 约束　⏳

- **落点：** `kiana-core/src/projection.rs`、`lifecycle.rs`；关联 `P0-B-01`、`P0-G-04`、`H27`。
- **动作：** 统一 Continue 的新 turn 语义、terminal 不复活、相同 terminal 幂等、不同 terminal conflict；补 `AwaitingApproval`、`Cancelling`、`Paused/NeedsRecovery`。
- **先拒绝：** `terminal_run_cannot_resume_without_new_turn_rule`、`conflicting_terminals_fail_closed`、`post_terminal_effect_is_not_success`。
- **成功/回归：** authorized→queued→running→approval→running→terminal、cancel race、continue、legacy event 顺序均产生同一 RunState。

<a id="step-er-09"></a>



##### ER-09 — Invocation/Execution/Attempt 投影　⏳

- **落点：** `kiana-core/invocation_projection.rs`、`dispatch.rs`、`capabilities.rs`；关联 `CP-13/14/20`、`CAP-24`。
- **动作：** 为每个 Invocation 折叠所有 execution/attempt，保存 action digest、permit、executor、started、effect_known、stop_confirmed、result/artifact refs；明确 terminal precedence。
- **先拒绝：** `dispatch_without_result_is_unknown`、`attempt_digest_mismatch_is_conflict`、`multiple_terminal_results_are_not_merged`。
- **成功/回归：** success/fail/cancel/unknown、late result、duplicate result delivery、retry attempt 和 pre-dispatch failure 均能从事件重建。

<a id="step-er-10"></a>



##### ER-10 — Approval、Budget、Lease 和 pending projection　⏳

- **落点：** `kiana-core/approvals.rs`、`cell_registry.rs`、`sessions.rs`、`kiana-daemon/journal_approvals.rs`；关联 `P0-F-01..03`、`P1-C/D/K5`。
- **动作：** 从事件重建 pending approval、reserved budget、path/resource lease、Cell 状态；把 cache miss 与真实不存在区分；所有消费、释放、fence 都有版本条件。
- **先拒绝：** `restarted_pending_is_not_auto_authorized`、`expired_approval_is_not_consumable`、`stale_lease_cannot_release_new_owner`。
- **成功/回归：** 新进程 list pending 与旧进程一致；一次消费、取消、过期、资源释放和父 Cell cascade 可重放。

<a id="step-er-11"></a>



##### ER-11 — Receipt DTO、redacted view 与 source cursor　⏳

- **落点：** `kiana-core/receipts.rs`、`kiana-protocol`、query ports；关联 `P1-J8-01`、`P2-M2/M4/M5`、`CAP-25`。
- **动作：** 将当前 receipt helper 收敛成版本化 `RunReceipt`/`ExecutionReceipt`/`CommandReceipt` DTO；每个字段标明 source event/artifact、redaction、missing/unknown 和 owner。
- **先拒绝：** `receipt_does_not_invent_success_from_missing_terminal`、`receipt_owner_mismatch_is_denied`、`artifact_read_failure_is_visible`。
- **成功/回归：** 同一 ledger 在 CLI/Web/Workbench/Desktop 产生同一安全 projection；跨 restart receipt 可查但不触发执行。

<a id="step-er-12"></a>



##### ER-12 — Cost、files、model turns 与 evidence aggregation　⏳

- **落点：** `receipts.rs`、`artifacts`、`usage`、workspace changeset；关联 `P1-K5-01`、`P2-K4-01`、`CO-24`。
- **动作：** 仅从 committed model/effect/changeset/artifact facts 汇总 cost、files_changed、model turns、memory hits、verification；外部账单必须带 provider receipt ref，估算值单独标记。
- **先拒绝：** `uncommitted_output_is_not_cost_or_file_fact`、`provider_estimate_cannot_settle_budget`、`redacted_artifact_is_not_original_evidence`。
- **成功/回归：** duplicate events、partial output、patch rollback、disk full、artifact revoke 后 Receipt 仍可解释且 limitations 完整。

#### C. 执行结果与事实回灌

<a id="step-er-13"></a>



##### ER-13 — 统一 result commit 和 result delivery　⏳

- **落点：** `kiana-core/dispatch.rs`、`capabilities.rs`、`runner` adapter；关联 `CP-05/14`、`H11/H13`、`CAP-24`。
- **动作：** 将 direct、Harness、approval-resume 三路径收敛到同一个 prepare→dispatch→result commit→delivery handler；result commit 先于 runner callback，delivery 用稳定 command id 去重。
- **先拒绝：** `result_delivery_without_committed_fact_is_denied`、`same_result_delivery_advances_harness_once`、`late_result_cannot_cross_cancel_fence`。
- **成功/回归：** broker success/failure/unknown、runner callback 失败、重复 callback、daemon crash window 都返回可重建结果。

<a id="step-er-14"></a>



##### ER-14 — Effect Receipt 与外部 provider receipt　⏳

- **落点：** `kiana-capability-broker`、connector/provider ports、`kiana-domain`；关联 `CAP-22/28/29`、`P4-K8-01`。
- **动作：** 定义 `EffectObservation`（provider receipt id、remote status、observed_at、query digest、evidence refs）；没有 provider query/idempotency 的 connector 默认只能返回 Unknown。
- **先拒绝：** `provider_timeout_is_not_no_effect`、`remote_receipt_owner_or_audience_mismatch_is_denied`、`non_idempotent_connector_retry_is_denied`。
- **成功/回归：** 已确认 success/failure/no-effect、远端重复 receipt、认证过期、断线和查询失败均正确折叠。

<a id="step-er-15"></a>



##### ER-15 — Hook、MCP、Memory、Patch 结果统一边界　⏳

- **落点：** `kiana-daemon/harness_*`、`apply_patch.rs`、`mcp_stdio.rs`、memory；关联 `CAP-13/18/19/21/22`、`H29/H30`。
- **动作：** 所有 adapter 返回统一 bounded result + effect/stop metadata；stdout/stderr、MCP frame、memory commit、patch changeset 在 result commit 前完成自身 durable boundary。
- **先拒绝：** `hook_mutation_reauthorizes`、`mcp_disconnect_after_send_is_unknown`、`memory_write_cancel_has_no_orphan_writer`、`patch_partial_commit_is_not_completed`。
- **成功/回归：** 每种 adapter 的结果在 RunReceipt 中有相同 Invocation shape；旧五工具 cassette 保持行为。

<a id="step-er-16"></a>



##### ER-16 — 终态事件唯一性与 terminal 必达　⏳

- **落点：** `kiana-core/events.rs`、`lifecycle.rs`、`kiana-daemon` shutdown；关联 `P4-J7-03`、`CP-15/16`、`H05/H33`。
- **动作：** terminal event 以 run turn 的稳定 command/idempotency key CAS 写入；写入失败返回 Unknown 并保留资源 quarantine；shutdown 先 flush journal 再报告终态。
- **先拒绝：** `terminal_append_failure_never_returns_completed`、`duplicate_terminal_is_idempotent`、`shutdown_does_not_drop_terminal_event`。
- **成功/回归：** normal completion、cancel、failure、result unknown 在 crash points 前后最多一个 terminal kind，且能查询。

#### D. Checkpoint、恢复和资源 fencing

<a id="step-er-17"></a>



##### ER-17 — Serializable RunSnapshot 与 pending writes　⏳

- **落点：** `kiana-domain/capabilities.rs`、`kiana-core/recovery.rs`、`kiana-runner/harness.rs`；关联 `P0-F-03`、`P2-J5-01`、`H22/H24`。
- **动作：** 明确 snapshot schema、previous snapshot/cursor、runner state digest、ContextPlan source refs、pending invocation、Cell/resource state；只保存 committed state，不保存不可验证 callback/secret。
- **先拒绝：** `snapshot_digest_or_scope_mismatch_is_denied`、`snapshot_with_redacted_input_is_non_resumable`、`snapshot_after_terminal_is_not_restorable`。
- **成功/回归：** approval pause、compact boundary、cancel pause、provider partial response、large state limit 和 schema migration 可加载或明确暂停。

<a id="step-er-18"></a>



##### ER-18 — Workspace checkpoint transaction 与 restore evidence　⏳

- **落点：** `kiana-daemon/workspace_checkpoints.rs`、`apply_patch.rs`、ArtifactStore；关联 `P2-K4-01`、`CAP-10/16`。
- **动作：** 捕获文件 identity/hash/mode/size，restore 前建立可回滚 transaction；提交和回滚都生成 changeset/evidence，外部 writer 变化使 restore 停止。
- **先拒绝：** `restore_symlink_or_out_of_scope_path_is_denied`、`workspace_revision_changed_during_restore_is_denied`、`rollback_failure_is_unknown`。
- **成功/回归：** empty/new file、rename、untracked file、disk full、crash at rename/fsync、concurrent writer 都不覆盖未授权内容。

<a id="step-er-19"></a>



##### ER-19 — Worker/process handle 与 fencing token　⏳

- **落点：** `kiana-core/sessions.rs`、`kiana-daemon/execution_*`、backend supervisor；关联 `CP-12/16/17`、`CAP-12/17/27`。
- **动作：** 将 process group/job/container handle、owner、lease、attempt、fencing token 和 stop report 持久化；reconnect/stop 必须核验 handle identity，TTL 不能单独证明已停止。
- **先拒绝：** `stale_handle_cannot_stop_new_execution`、`child_escape_or_leader_exit_is_unknown`、`lease_expiry_does_not_release_live_process`。
- **成功/回归：** process tree、setsid、PTY/pipe、container child、daemon crash 和 bounded reap 均有真实 stop/effect report。

<a id="step-er-20"></a>



##### ER-20 — Restart projector 与默认暂停　⏳

- **落点：** `kiana-core/recovery.rs`、`projection.rs`、`kiana-daemon/lib.rs`；关联 `CP-18/19`、`P2-M5-02`、`H24/H25`。
- **动作：** 启动时依次 validate journal→load projection→rebuild pending/unknown→fence resources→open read-only recovery state；不自动 invoke runner/provider/broker。
- **先拒绝：** `restart_never_auto_resumes_pending_run`、`corrupt_journal_does_not_boot_empty`、`rebuild_does_not_issue_permit`。
- **成功/回归：** drop host/new host、multiple pending approvals、orphan dispatch、projection checkpoint mismatch、valid empty store 的结果可区分。

<a id="step-er-21"></a>



##### ER-21 — Explicit resume preflight and claim　⏳

- **落点：** `kiana-core/recovery.rs`、protocol command、CLI/Web routing；关联 `P0-G-03`、`CP-19`、`H14/H19`。
- **动作：** resume 读取 exact snapshot cursor、owner/role/sandbox/path/epoch/approval，CAS 写 `run.resume_prepared`，安装 runner 前再核验 authority；重复 resume 返回原 command receipt 或 conflict。
- **先拒绝：** `resume_context_override_is_denied`、`resume_after_data_revocation_is_denied`、`resume_same_snapshot_twice_is_idempotent`。
- **成功/回归：** approval pending、no-pending continuation、new turn、stale snapshot、concurrent resume 和 runner restore failure 都有稳定响应。

<a id="step-er-22"></a>



##### ER-22 — Cancel recovery and stop confirmation　⏳

- **落点：** `kiana-core/lifecycle.rs`、`dispatch.rs`、daemon supervisor；关联 `P0-J1-01..04`、`CAP-17`。
- **动作：** cancellation generation 覆盖 queued/pending/started；为每个 execution 保存 stop request/ack/observed process state；只在全部必要 stop/effect 事实确认后写 `run.cancelled`。
- **先拒绝：** `cancel_before_dispatch_has_zero_effect`、`cancel_after_dispatch_without_stop_is_unknown`、`late_result_cannot_resurrect_run`。
- **成功/回归：** cancel 与 approval、spawn、handler result、terminal append、daemon shutdown 的屏障注入测试。

<a id="step-er-23"></a>



##### ER-23 — Unknown incident 与 RecoveryPlan　⏳

- **落点：** `kiana-domain/platform.rs`、`kiana-core/platform.rs`、Human Inbox；关联 `P2-K6-01`、`CO-32`。
- **动作：** 对 journal failure、provider timeout、MCP crash、disk full、orphan process、data revocation 生成 Incident/RecoveryPlan；observed_state 和 last cursor 来自投影，safe/forbidden action 只增不减。
- **先拒绝：** `unknown_without_incident_is_denied`、`recovery_plan_cannot_self_approve`、`forbidden_action_cannot_be_removed`。
- **成功/回归：** Proposed→Approved→Executing→Verified/Failed/Abandoned 每次转移有 event、actor、evidence 和 idempotency key。

<a id="step-er-24"></a>



##### ER-24 — Reconciliation commands and evidence　⏳

- **落点：** `kiana-core/platform.rs`、protocol command、connector/provider ports；关联 `CP-20/23`、`P2-K6/K7`。
- **动作：** 实现 `reconcile` 的 observed_succeeded/observed_failed/no_effect 三类明确结论；每个结论绑定 provider/OS/file/人工 evidence ref 和原 source event，runtime outcome 不被原地改写。
- **先拒绝：** `reconcile_without_evidence_is_denied`、`evidence_for_other_incident_is_denied`、`reconcile_is_not_retry`。
- **成功/回归：** 重复 reconciliation 返回原 receipt；查询成功、查询失败、证据撤销、authority/data epoch 变化均不越权。

<a id="step-er-25"></a>



##### ER-25 — Retry policy and new attempt　⏳

- **落点：** `kiana-domain` retry/timeout contracts、`kiana-core/dispatch.rs`；关联 `P2-K6-01`、`H05/H07/H08`。
- **动作：** 持久化 max attempts/backoff/deadline/error classifier/idempotency requirement；仅在 effect 明确未发生且 policy 允许时创建新 attempt，重新做 authority/approval/lease preflight。
- **先拒绝：** `unknown_effect_never_auto_retries`、`policy_denied_never_retries`、`retry_exhaustion_is_terminal_failure_or_unknown`。
- **成功/回归：** transient before dispatch、known no-effect failure、timeout、attempt exhaustion、backoff deadline 和 duplicate idempotency key。

#### E. Query、入口和业务接线

<a id="step-er-26"></a>



##### ER-26 — Cursor query、snapshot 和慢消费者　⏳

- **落点：** `kiana-eventlog` cursor API、`kiana-daemon/run_stream.rs`、protocol；关联 `P4-J7-02`、`P2-M2/M5`。
- **动作：** 使用 epoch+logical cursor；snapshot 返回 source cursor、projection version、terminal/unknown；SSE/Web/CLI 断线只补读，不创建新 run。
- **先拒绝：** `cursor_epoch_mismatch_requires_snapshot`、`slow_consumer_cannot_block_event_commit`、`ui_event_cannot_override_receipt`。
- **成功/回归：** duplicate page、gap、late terminal、multi-tab、daemon restart、large page 和 consumer cancellation。

<a id="step-er-27"></a>



##### ER-27 — 四入口统一只读 receipt/recovery commands　⏳

- **落点：** `kiana-entrypoints`、`kiana-client/protocol`、`DaemonHost`；关联 `CP-21/22`、`P2-M2..M7`、`CO-40/41`。
- **动作：** CLI/Workbench/Web/Desktop 只调用同一 DaemonHost query/command；展示使用安全 DTO；resume/cancel/reconcile/restore 都回 ControlPlane，不在入口执行副作用。
- **先拒绝：** `entrypoint_cannot_parse_or_mutate_eventlog`、`ui_supplied_owner_or_scope_is_denied`、`receipt_query_never_starts_execution`。
- **成功/回归：** 四入口同一 run 的 cursor、receipt、pending 和 incident 一致，权限/错误码一致。

<a id="step-er-28"></a>



##### ER-28 — CompanyOS / Workflow / Artifact 业务引用　⏳

- **落点：** `kiana-workflow/durable.rs`、`kiana-core/company*.rs`、`artifacts.rs`；关联 `P2-J5`、`P3-I-03..06`、`CO-24..42`。
- **动作：** Company command、Workflow node、Review、Delivery、ClosingReceipt 只引用运行时 Receipt/EvidenceBundle；业务验收和 runtime terminal 分开，workflow node 的 Unknown 升级 Incident。
- **先拒绝：** `company_success_cannot_be_inferred_from_run_completed`、`workflow_replay_does_not_execute_effect`、`closing_without_evidence_is_denied`。
- **成功/回归：** Builder→Review→Acceptance→Delivery→Close 的每个跨边界引用可从事件和 artifact 重建。

<a id="step-er-29"></a>



##### ER-29 — Data governance、retention 和 deletion propagation　⏳

- **落点：** `kiana-core/data_governance.rs`、Artifact/Memory/Query stores；关联 `P2-K7-01`、`CP-25/26`。
- **动作：** Receipt 保留审计 metadata 与 payload refs 分离；删除/撤销传播到 event projection、artifact、cache、memory/index，并记录 data epoch；原始不可变事件只按既定密级加密/封存，不直接篡改。
- **先拒绝：** `revoked_data_is_not_returned_by_receipt`、`deletion_does_not_leave_untracked_projection_copy`、`receipt_redaction_is_not_authorization`。
- **成功/回归：** source revoke、artifact expiry、memory deletion、cache rebuild、audit-only retention 和 restart。

<a id="step-er-30"></a>



##### ER-30 — Health、metrics、trace correlation and operator evidence　⏳

- **落点：** `kiana-core` observability、daemon health、Receipt metadata；关联 `P1-J8-01`、`CAP-34`。
- **动作：** 记录 append/flush/projector/recovery latency、queue depth、last durable cursor、unknown count、orphan count、stop confirmation、artifact bytes；trace/span 只引用 correlation/causation，不写秘密。
- **先拒绝：** `metrics_cannot_claim_effect_success`、`health_unknown_is_not_healthy`、`telemetry_secret_scan_blocks_publish`。
- **成功/回归：** normal、slow disk、projector lag、unknown、recovery and operator query 均有可解释指标和限制。

#### F. 故障矩阵和交付门

<a id="step-er-31"></a>



##### ER-31 — Crash-point and fault-injection matrix　⏳

- **落点：** EventStore、ControlPlane、Broker、Runner、workspace、provider fixtures；关联 `CAP-26`、`CAP-34`、`H25/H35`。
- **动作：** 在 prepare 前/后、permit consume、spawn、stdin partial write、patch rename/fsync、stop/reap、result commit、result delivery、projection checkpoint、artifact publish、cleanup 各点注入 crash/timeout/disk full。
- **先拒绝：** 每个点都必须证明 `no duplicate effect`、`no false success`、`unknown is queryable`、`resources remain fenced`；不能用固定 sleep 或 mock `Ok` 抵扣。
- **成功/回归：** 新进程按 journal 只做一次安全恢复；同一 fault seed 重放得到同一分类和 Receipt limitations。

<a id="step-er-32"></a>



##### ER-32 — Property/conformance tests for adapters　⏳

- **落点：** `kiana-eventlog`/`kiana-core` tests、Memory/JSONL adapter conformance；关联 `P1-L1-01`、`P2-K6-01`。
- **动作：** 共享 fixture 验证 CAS、idempotency、cursor boundary、schema migration、redaction、projection equivalence、unknown/terminal invariants；Memory 只能证明逻辑，JSONL 单独证明 durable adapter 条件。
- **先拒绝：** `adapter_capability_mismatch_is_denied`、`memory_adapter_cannot_claim_durable`、`property_shrinking_never_turns_failure_into_success`。
- **成功/回归：** 随机事件序列、重复命令、并发冲突、截断尾、未知字段和 projector restart 收敛到同一结果。

<a id="step-er-33"></a>



##### ER-33 — Performance、容量和迁移演练　⏳

- **落点：** EventLog benchmark、projection rebuild、artifact quota、migration tooling；关联 `CP-27/28`、`CAP-34`。
- **动作：** 建立事件/frame/page、flush、rebuild、receipt query、artifact bytes、queue depth 和恢复时延基线；验证 journal rotation/archive、schema upgrade、downgrade read-only 和容量拒绝。
- **先拒绝：** `journal_full_does_not_append_partial_frame`、`migration_unknown_version_fails_closed`、`rebuild_over_quota_is_bounded`。
- **成功/回归：** 目标机器在配额内完成；超限产生稳定 error/Unknown 和 operator action，不隐式删除事实。

<a id="step-er-34"></a>



##### ER-34 — Local durable gate　⏳

- **落点：** release/smoke、`CURRENT_STATUS.md`、roadmap status；关联 `CAP-26`、`P2-K6`。
- **动作：** 在稳定快照上串行运行 eventlog/core/daemon focused tests、workspace check/fmt/clippy、golden/workbench/P0 smoke；采集 event/frame/artifact hashes、process/file/lock/usage 事实。
- **验收：** deny→success→restart→receipt→unknown reconciliation 的五工具本地闭环全部命中；不得把历史 CI 或 moving WIP 的 `cargo check` 当本次证据。
- **交付：** 只提升实际覆盖的 `feature_status`/`proof_level`，补 `CURRENT_STATUS` 证据块和限制。

<a id="step-er-35"></a>



##### ER-35 — Cross-entry and CompanyOS gate　⏳

- **落点：** CLI/Workbench/Web/Desktop、Workflow、Company business tests；关联 `P2-M*`、`CO-40..48`、`CAP-34`。
- **动作：** 用同一持久 EventStore 验证四入口查询、approval/resume/cancel/reconcile、Review/Delivery/Close 的引用一致性；证明 UI 断线只读补游标。
- **验收：** 入口不产生第二执行循环；业务 Delivery/ClosingReceipt 不越权改写 RunReceipt；未知、撤销、删除和旧 schema 在每个入口可见。
- **交付：** 记录跨入口 command/event/receipt correlation、fixture hash、精确命中数和不适用组合。

<a id="step-er-36"></a>



##### ER-36 — Physical/live boundary and handoff　⏳

- **落点：** 目标平台 backend、live provider/connector、operator runbook；关联 `CAP-27..34`、`P4-J7-31`。
- **动作：** 在具备 bwrap/userns/cgroup、目标 OS、隔离测试账户和 provider receipt 的环境分别验证；区分 local durable、live provider 和 physical effect，保留失败和未支持矩阵。
- **先拒绝：** 缺 backend/credential/remote query 时只能证明 deny；不能 fallback 到宿主、不能把 mock receipt 写成 live、不能把 build 通过写成 physical。
- **交付：** 每个组合附 recovery/incident/cleanup 证据、权限和数据边界、版本与限制；最后由 reviewer 复核而非模型自述完成。

### 23.6 统一验收场景

以下场景贯穿 ER 卡，先覆盖拒绝再覆盖成功。每个场景应同时检查 EventLog、Projection/Receipt、实际文件/进程/远端 fixture 三者；只看到返回值不算通过。

| 场景 | 必须观察 | 关联步骤 |
|---|---|---|
| 同 command 重试、不同 digest、两个 writer CAS | 原 receipt replay；digest mismatch/conflict；无第二事件 | ER-04/05/32 |
| approval 等待后 path/role/epoch/schema 变化 | 原 approval 失效；没有 handler/spawn/effect | ER-10/21 |
| permit consume、spawn、handler start、result commit 各点崩溃 | 新进程只重建；未确认 effect 为 Unknown；无重复 effect | ER-09/20/31 |
| cancel 与 approval/spawn/result/terminal 竞态 | not-executed 与 stop-confirmed 分开；不能假成功 | ER-16/22 |
| provider/MCP timeout 或 disconnect | 已发送业务请求不自动 retry；receipt/incident 可对账 | ER-14/15/24/25 |
| patch rename/fsync、外部 writer、workspace restore | revision/identity 不符拒绝；不覆盖新内容 | ER-18/31 |
| journal malformed/checksum/torn tail/disk full | 可修复范围明确；不可修复 fail-closed；不变空 store | ER-05/33 |
| secret/redaction/data deletion | 事件、Receipt、artifact、cache、memory 均无未治理副本；resumable 正确降级 | ER-03/12/29 |
| projection checkpoint、cursor、慢 Web consumer | checkpoint 失效可重建；cursor gap 要 snapshot；写入不阻塞 | ER-07/26 |
| Company Review/Delivery/Close | 业务成功引用 Evidence/Receipt；不把 run completed 当 Outcome | ER-28/35 |

### 23.7 执行命令与证据回填

以下命令是实施时的建议入口，按当前卡和实际 target 选择；未命中的测试名不能写成通过。daemon/control-plane 测试必须串行。

```bash
# 事实存储和 domain contracts
cargo test -p kiana-domain --locked --offline
cargo test -p kiana-eventlog --lib --locked --offline -- --test-threads=1
cargo test -p kiana-eventlog --locked --offline -- --test-threads=1

# projection / receipt / recovery / authorization
cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-core --test dependency_boundaries --locked --offline
cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1

# workspace gates after focused tests are green
cargo check --workspace --locked --offline
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
bash scripts/release-smoke.sh
bash scripts/harness-golden-smoke.sh
bash scripts/v10-workbench-smoke.sh
bash scripts/v10-p0-closeout-smoke.sh
```

每张 ER 卡完成时回填：

```text
step: ER-xx; linked P / CP / H / CAP / CO units: ...
source_snapshot: exact commit plus relevant WIP file hashes
worktree_status: changed paths and concurrent/unrelated changes
command_argv: exact command, filter, matched test count, test-thread setting
cwd/environment: OS/kernel, toolchain, EventStore adapter/capabilities, backend,
                 sandbox, feature flags, quotas, provider/fixture version
fixture/cassette: event/frame/artifact/provider fixture hash and fault point
exit_code: each command; baseline failures separated from this step
status change: feature_status before -> after for exercised behavior only
proof-level change: source/local_behavior/durable/live/physical, exactly as observed
limitations: unsupported backend, unverified effect, projection lag, data bounds
reviewer: named reviewer; self-review marked as self-review
```

本专项本次只完成研究和路线图追加：没有运行产品测试、没有提升任何原 P/CP/H/CAP/CO 状态，也没有提交、推送、合并或修改产品源码。后续实施必须从 `ER-00` 重新采集稳定快照；WIP 中已有的 journal/recovery 方法只能作为待验收实现，不得直接抵扣本节步骤。

---

返回：[路线图总图与当前窗口](../roadmap.md#appendix-navigation) · [文档总入口](../README.md)
