# OA-00 Observability / Audit 现状 inventory

> 快照日期：2026-09-15。本文是 `OA-00` 的 source-only inventory，不是
> `ObservabilityPort`、Audit、Metric、Trace 或 Health 管线的交付声明。它记录事实、
> 派生视图和尚未存在的合同，避免把显示、统计或评测快照误当成授权或业务结果。
> 本轮不在本地运行测试；`observability_baseline` 仅由 GitHub Actions 执行。
> OA-01/OA-02 后续引入的 domain contract 与 correlation 模块会使导出/注册表源码发生
> 预期漂移；本表继续锁定 OA-00 所覆盖的旧边界文件，并把后续 overlay hash 单独列明，
> 扩展时必须在同一提交中更新 hash 和迁移说明。

## 1. 快照、范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap 卡 | [`OA-00`](../roadmap.md#step-oa-00) |
| source snapshot | `0fb757588a232333ecb0e8304215e8c247ad0d8`（SW-00 已推送的干净基线） |
| feature_status | `partial`：事实/收据/展示的现状已盘点；正式 observability/audit 目标仍未实现 |
| proof ceiling | `source`；静态检查与源码索引不提升 `local_behavior`、`durable`、`live` 或 `physical` |
| 事实源 | `ControlPlane → EventStore → RuntimeEvent/CommandReceipt`；Receipt 和所有信号均须标为派生或观察 |
| 范围 | `docs/module-map.md`、`CURRENT_STATUS.md`、domain event/journal/contracts、core event/receipt/projection/recovery/versioning、EventLog adapters、daemon RunStream/Host、现有 `P1-J8-01`/`ER-30` |
| 本轮不做 | 不新增 schema、sink、exporter、projector、health probe、audit query、第二事实源或第二执行循环；这些属于 OA-01+ |

源码快照 hash（后续 OA 步骤改动这些文件时，必须重新盘点并更新护栏）：

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Runtime event shape | `kiana-domain/src/states.rs` | `dca28dc71ef75e5dd92a25bed399349ca286bc7c5c0e514e11de20d6d1491ae3` |
| Transition contract | `kiana-domain/src/journal.rs` | `4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48` |
| Schema/ID registry | `kiana-domain/src/contracts.rs` | `1fe78e8faecb8b3459a5f086699ce4d3a0a3f6660e780fdb485b2dac4f46b7eb`（OA-03 注册表扩展） |
| Domain exports | `kiana-domain/src/lib.rs` | `ac35afaebdb152eddb0898f7e9a28a886d17abdc35643d71cdfb2267e3f731a4`（OA-04 audit reducer 导出） |
| Audit taxonomy/reducer（OA-04） | `kiana-domain/src/audit.rs` | `b17e84def5825be9c2fbdfa704ffe12f9502821325e0c2f66e977b1d72aed1e1` |
| Core audit facade（OA-04） | `kiana-core/src/audit.rs` | `9c444ff6a125a90ac6ba25c1756802e826c40e681ce24f21645f1f08eff9ee22` |
| Correlation links（OA-02 overlay） | `kiana-domain/src/correlation.rs` | `6fabe5e7cb8d2c86604738108eebcea6274d36306afac197a6115d13b1bfa951` |
| Event construction/redaction | `kiana-core/src/events.rs` | `7d1852ad4a9d93792256288a01a25e06c677e0f6641274b2575b718c87020679` |
| Receipt projection | `kiana-core/src/receipts.rs` | `dda33c346b1ffd94389093e2856f99433ea083a3c61b35cf562484f9f4bc7e1e` |
| Run/invocation projection | `kiana-core/src/projection.rs` | `20d84eb8fa0ac77ac85b48acb10e2fcc78214cc1c3c10be339b540230b1ff68` |
| Recovery/checkpoint | `kiana-core/src/recovery.rs` | `f964c69bb32e940782ef52b399afd9ad6902778b4900f6b7164cb79f3dcc7d26` |
| Lifecycle receipt write | `kiana-core/src/lifecycle.rs` | `346ecb8ea2879b24a53a2ee238bb46c3f7793ffa6211c126ec86ad49c47bc237` |
| Golden trace/replay | `kiana-core/src/versioning.rs` | `679c88757469189f93b20163bbfccb9d65a161ab24ec9f6d9e73a483586108ab` |
| Review/Outcome boundary | `kiana-core/src/collaboration.rs` | `49e61cf89add855ca4fc3687104d666388477848d63397b9ea50b0404021dabf` |
| EventStore facade | `kiana-eventlog/src/lib.rs` | `61aa7bcc9c221474d3ffa75a323ba7931f1da8e7ce96107da1d1d7cb7fabd20c` |
| EventStore append planning | `kiana-eventlog/src/event_store_core.rs` | `506a8222a757a89e6516a12b6260daa7f35af2b3da49c17a23ca7cdbcc9d0b1e` |
| Journal state/replay | `kiana-eventlog/src/journal_core.rs` | `84ef64bc8208b2ead45d1d05feb0265e94129b898cd67d04f5ed189d388bba56` |
| JSONL adapter | `kiana-eventlog/src/jsonl.rs` | `f549e5de5bc4e197f268b865327bd1d0d7a4c10208daa3e7c13ceb68188e5c5f` |
| Memory adapter | `kiana-eventlog/src/memory.rs` | `bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9` |
| RunStream projection | `kiana-daemon/src/run_stream.rs` | `750bb78e46e5f1c42d3a773d0afcf468cc6713032ef47e30ffa100f3313db5c6` |
| Daemon composition | `kiana-daemon/src/lib.rs` | `f262e808ab239eed0e01c1e28aef75cb5d78b819d809c1d07592ab34d367d281` |
| Port contract | `kiana-ports/src/lib.rs` | `557332aa2115b8c98d7aa261eddb3549c07b6805dd53aa41bf1b3ccc8e0ba723`（OA-02 port overlay） |

## 2. Signal matrix

| 信号/对象 | 当前 owner 与接线 | 事实还是派生 | 当前限制 / 迁移 owner |
|---|---|---|---|
| `RuntimeEvent` | `kiana-domain::RuntimeEvent` 是 `kind: String + data: Value`，由 `ControlPlane::append_event`/`record_terminal_event` 构造 | EventLog 中的事实载荷；不是完整 signal schema | 没有 event-kind registry、schema owner、timestamp、correlation/causation 或 DataClass；OA-01/OA-02 |
| `TransitionBatch` / `CommandReceipt` / `CommitOutcome` | domain journal 合同 + `EventStorePort`；JSONL/Memory journal 做 CAS、幂等和 cursor | 命令提交事实 | receipt 证明提交边界，不证明 dispatch/effect/业务 Outcome；EventStore port 本身不承诺 durable；OA-06/ER-02 |
| `MemoryEventLog` | `kiana-eventlog::MemoryEventLog` | 进程内事实缓存 | `durable_commits=false`，不能作为 durable observability evidence；ER/PD |
| `JsonlEventLog` | JSONL frame、锁、checksum/torn-tail/Unknown 路径 | 本地 EventLog 事实 adapter | `cfg!(unix)` 能力和源码不等于重启/物理 durability 证明；没有 observability projector；ER-31/PD-27 |
| Run / Invocation projection | `kiana-core::projection` 从 run/approval/capability 事件折叠，内存 map 仅缓存 | 派生状态 | 缺正式 projection version、source cursor、lag/checkpoint；缓存不是事实；OA-06/OA-10 |
| `CommandReceipt` → Run receipt | `kiana-core::receipts::receipt_from_events` 从过滤后的 EventLog 生成，并最终再做 redaction | 派生 Receipt | 缺顶层 `source_cursor`/`source_event_ids`/`projection_version`/`limitations` 合同；Receipt ≠ business Outcome；ER-01/ER-30 |
| `run.receipt` event | lifecycle 在成功路径把派生 receipt 再写入 EventLog | 现有事件事实中的 shadow copy | 易使 receipt 看似第二事实源；必须区分原始事实与可重算 projection；OA-06/ER-02 |
| `ResultUnknown` | receipt/projection 对缺 terminal、矛盾 terminal、读取/投影异常保留 `ResultUnknown` | 事实驱动的失败分类 | 没有统一 Incident/Recovery/Audit projection 与 operator query；ER-30/OA-10/OA-19 |
| RunStream / SSE / Workbench | `RunStreamBus` + `StreamEventStore`；只在 committed fresh append 投影，replay 不重复 | 进程内、bounded、best-effort 展示 | broadcast send 可丢、epoch/cursor 在内存中；gap/terminal replay 不是 EventLog 事实、授权或 Outcome；OA-13/OA-24 |
| transcript / UI timeline / cache | 入口与 `kiana-screens`/context index 读取 RunStream 或缓存 | 派生视图/加速缓存 | 不能作为状态、审计、成功、健康或恢复依据；OA-16/OA-20/OA-24 |
| model turn / usage | `run.model_turn`、`run.usage` 等事件携带 provider/model/usage/elapsed/error；`UsageRecord` 另有 domain 类型 | 事件中局部事实 + 展示投影 | `run.usage`、`model.usage`、`provider.usage`、`usage.recorded`、`run.model_turn` 语义重复；无 MetricCatalog/单位/低基数 labels；OA-08/OA-12 |
| Business Metric / Incident | Company closeout 的 `MetricObservation`、`Incident` 和 platform `FailureIncident` | Company/故障领域事实，不是 OA signal contract | 不能把业务指标或 Incident DTO 当 runtime MetricSnapshot/HealthSnapshot/AuditRecord；OA-04/OA-11/OA-19 |
| `PreparedModelCall::audit()` | provider/model 请求的安全摘要 | 局部诊断摘要 | 不是 `AuditRecord`，无 source event/cursor、decision taxonomy、retention/query boundary；OA-03/OA-04 |
| `golden_trace.captured` / `trace.replay` | `kiana-core::versioning` 复制 run events/receipt，校验 owner/project/hash/revocation，replay 明确 `side_effects=false`/`provider_calls=0` | EventLog 中的评测快照与只读 replay | golden trace ≠ telemetry trace；trace 不得授权、恢复或替代 event order；完整 events/receipt shadow copy 的 retention 需迁移；OA-02/OA-14/OA-21 |
| Operational log | 没有 `ObservabilityPort`/结构化 log sink；OA-03 提供 `RedactionProfile`/bounded encoder | profile/encoder source 已实现；runtime sink 未实现 | exporter、脱敏失败、队列、flush/关闭 ack 未建模；OA-05/OA-13 |
| Metric | OA-01 已注册 `MetricCatalog`/`MetricPoint` domain contract，OA-03 提供 Metric profile boundary；仍没有 reducer/sink | schema/encoder source 已实现；runtime 未实现 | 不得把 usage counter、UI cursor 或 Company metric 当 canonical runtime metrics；OA-10/OA-12 |
| Trace / span | OA-01 已注册 `TraceSummary`，OA-02 已注册 `CorrelationContext`/`TraceRef`/`SpanRef` 与 link，OA-03 提供 Trace bounded encoder；没有 `TraceSink`；现有 trace 名称属于 golden replay | schema/correlation/encoder source 已实现；runtime 未实现 | trace 只可关联，不可作为 actor/authority/policy/approval；OA-07/OA-14 |
| Audit | OA-04 在 `kiana-domain/src/audit.rs` 固定 taxonomy，并由 `kiana-core/src/audit.rs` 从带 cursor/source binding 的 committed `RuntimeEvent` 派生 `AuditRecord`；OA-02 causation/command/attempt typed link、OA-03 Audit redaction boundary 继续适用；仍没有 `AuditQueryPort`/checkpoint projector | taxonomy/reducer source 已实现；runtime observer/query 未实现 | 模型/UI/plugin/exporter 不能伪造批准/完成/导出；未知 `audit.*`、缺 source/epoch、矛盾 decision、重复 source 记录 fail-closed；OA-05/OA-06/OA-15/OA-16 |
| Health / Incident signal | 没有 `HealthSnapshot`/`HealthProbePort`/degraded contract | 未实现 | stale、projector gap、unknown exporter、journal corruption 不能返回 healthy；OA-10/OA-11/OA-19 |

## 3. 先拒绝：已确认的风险与证据窗口

1. **显示不是事实。** RunStream、SSE、Workbench transcript、UI cursor、context/index
   cache 和模型自述均是派生视图；只有 EventLog/Transition/Receipt 可定位到事实游标。
   `RunStreamBus` 的 `epoch`、sequence、gap 和 terminal replay 只防止部分 stale/replay
   展示问题，不提供授权或业务成功证明。
2. **Receipt 不是 Outcome。** Receipt 从事件重算并被 redaction；`run.completed` 只说明
   当前运行事实完成。现有 review 路径在 `run.completed` 且 `files_changed` 非空时生成
   `pass`/accepted merge receipt，这不能证明现实交付、业务验收或外部效果，列为
   `receipt/terminal-as-Outcome` issue。
3. **Golden trace 不是 telemetry trace。** `golden_trace.captured` 嵌入完整 events 与
   receipt，是评测/重放快照，不是 W3C trace/span。虽然 replay 不调用 provider 且
   `side_effects=false`，它不能成为授权、恢复或审计事实。
4. **事实先于观察仍需完整收敛。** core 的 `append_event` 在构造事件前做通用 redaction，
   但 EventStore adapter 不负责 classification/redaction，现有 redactor 是有限的 key/marker
   规则且不可失败；所有直接 append writer、future sink/export 都要在 OA-03 统一收敛。
5. **命名和字段仍漂移。** OA-01 已固定 domain signal 的版本、digest、cursor、source event
   IDs 和 attribute 上限，但 RuntimeEvent/usage 仍同时出现多种 event kind，时长有 `elapsed_ms` 与
   `duration_ms`；没有注册表、单位、低基数 label、source cursor、data class、retention
   或 proof ceiling 字段。重复或私自命名的观测字段不得继续扩散。
6. **Unknown 必须保持 Unknown。** JSONL commit 在无法确认写入时返回 `CommitOutcome::Unknown`，
   receipt/projection 对缺失或矛盾 terminal 返回 `ResultUnknown`；EventLog worker queue 满时
   返回 `eventlog_worker_queue_full`，RunStream broadcast 可能丢 best-effort 投影。当前没有
   audit/telemetry 专用队列、flush ack、lag 或 incident recovery contract。
7. **历史证据不重写。** `ER-30` 与 `P1-J8-01` 仍是待实施卡；本 inventory 只记录其与当前
   源码的差距，不把 ER-00 的 EventLog 事实边界或任何既有局部测试升级成统一 observability
   runtime/durable 证明。

## 4. Owner、proof ceiling 与迁移清单

| 后续 owner | 迁移结果 | 现状 proof ceiling |
|---|---|---|
| OA-01 | 注册 observability/audit/metric/trace/health schema、版本和 unknown-field 策略 | `source`；四类 domain contract 已实现，HealthSnapshot/runtime adapter 仍未实现 |
| OA-02 | 服务端构造 CorrelationContext、TraceRef/SpanRef 和 causation/parent link | `source`；domain/port contracts 已实现，尚无 runtime ingress/span bridge |
| OA-03 | 统一 redaction/classification、bounded encoder 和 secret sentinel 全信号扫描 | `source`；profile/encoder 已实现，EventStore/各 runtime sink 尚未统一接线 |
| OA-04 | Audit taxonomy/Record reducer，从 committed security facts 派生 | `source`；domain/core reducer 已实现，尚无 EventLog commit observer、checkpoint、query 或 durable projection |
| OA-05 | Observability/Trace/Metric/Audit/Health ports 与 fake adapters | `source`；目前没有 sink 能力或 flush ack |
| OA-06 | commit observer 只通知 Committed，重放不重复通知，建立 projection cursor | `source`；RunStream wrapper 仅是局部前身 |
| OA-07–09 | run/provider/broker/approval/effect/stop 的 span 与 usage 生命周期 | `source`；现有 model-turn/usage 事件未形成统一 signal |
| OA-10–13 | Receipt/Health/Metric reducer、lag、队列背压与丢弃分类 | `source`；没有 runtime gauges 或 telemetry queue |
| OA-14–18 | trace exporter、Audit checkpoint/query/cursor/export | `source`；golden replay 不是 exporter，不能声称 durable/live |
| OA-19–21 | Incident/Recovery、retention/deletion 和 replay diagnostics | `source`；FailureIncident/Company Incident 不能代替 OA Incident |
| OA-22–28 | fault/eval/入口 parity/容量/local durable/live handoff | `source`；无本地 runtime 或外部 backend 证明 |

## 5. 现有测试边界（仅源码索引）

源码中已有 EventLog 的 Memory/JSONL 序列、幂等、重开和 Unknown 测试，以及 core 的
`event_stream_read_failure_prevents_append_and_runner_dispatch`、
`receipt_replays_result_unknown_without_claiming_success`、
`receipt_read_all_failure_fails_closed`、
`runner_delta_completion_and_receipt_are_redacted`、
`new_process_rebuilds_run_state_from_events_alone` 和
`new_process_rebuilds_invocation_state_from_events_alone` 等局部测试名。

这些测试仍属于各自 Event/Receipt/Runner 边界；它们没有共同的 observability schema、
Audit query、Metric reducer、Trace sink 或 Health projection，因此不能作为 OA-00 的统一
runtime 证据。本步只在 GitHub Actions 运行 source guard；本地未执行测试二进制。

## 6. OA-00 退出条件

OA-00 完成只表示：signal matrix、代码 owner、当前 proof ceiling、迁移清单和明确的拒绝
边界已入库；`source-only` 护栏会在后续源文件漂移或缺少这些边界声明时失败。它不表示
Audit/Metric/Trace/Health runtime 已实现，也不改变 `P1-J8-01`、`ER-30` 或任何历史 evidence block
的状态。OA-01 的 domain schema 叠加已单独记录；下一步按 roadmap 进入 OA-02 correlation/trace
references。

## 7. OA-01 叠加说明

OA-01 在 `kiana-domain` 注册 `observability.v1`、`audit-record.v1`、`metric-catalog.v1` 和
`trace-summary.v1`，并提供 `ObservabilityRecord`、`AuditRecord`、`MetricCatalog`、
`MetricPoint`、`TraceSummary` 及封闭状态/来源枚举。每个 contract 使用 `deny_unknown_fields`、
同 major 的 minor compatibility、非零 `source_cursor`、bounded `source_event_ids`/attributes
和 `sha256:` digest 校验；`MetricPoint::validate_with_catalog` 对未注册名称 fail-closed。

这只是 domain/schema source proof。没有新增 EventStore 写者、projector、sink、授权判断或
外部 exporter；schema 存在不代表 audit/metric/trace/health 的 runtime、durable、live 或
physical 证明。OA-00 的旧边界 hash 已在同一迁移序列中更新 `contracts.rs`/`lib.rs` 两项，
新 `observability.rs` 由 OA-01 的专项编译/CI 护栏负责。

## 8. OA-02 叠加说明

OA-02 在 `kiana-domain/src/correlation.rs` 注册 `kiana.correlation-context.v1`，提供严格
解析的 W3C `traceparent`、`TraceId`/`SpanId`、`TraceRef`/`SpanRef`、`SpanLink`、
`AttemptRef`、`CausationRef` 和服务端派生的 `CorrelationContext`。根上下文从已认证的
`RequestContext`、服务端解析的 scope 与 authority/data epoch 构造；外部 traceparent 只
形成 `ForeignParent` link，不能提供 actor、project、session 或权限。run→turn→invocation→
attempt 绑定和 command/attempt、scope、epoch 校验均 fail-closed；本地 child span 使用
parent ref，异步/recovery child 使用新 span + `FollowsFrom` link。

`kiana-ports` 的 `CorrelationContextPort`/`DomainCorrelationContextPort` 仅委托这些纯
domain 不变量，不访问 EventStore、Broker、Provider 或网络。这是 source/静态编译 proof；
没有新增 runtime ingress、span sink、授权路径或 durable/live/physical 证明。OA-03 继续
处理跨 signal 的统一 redaction/classification 和 bounded encoder。

## 9. OA-03 叠加说明

OA-03 在 `kiana-domain/src/redaction.rs` 增加 versioned `RedactionProfile`，按 log/metric/
trace/audit/export signal 声明 `DataClass`、最大字节数和最大嵌套深度，并绑定 canonical
profile digest。`encode_bounded_value`/`encode_bounded_text` 复用既有 `redact_value`/
`redact_text`，随后检查 NUL、深度、UTF-8/JSON 编码、大小和残余 secret marker；profile、
结构或文本任一校验失败都返回稳定错误，不提供原文 fallback。`secret_ref` 等受控引用
可以保留，secret/token/password/api-key/header 等值必须变为 `[REDACTED]`。

本步只交付 domain/profile/encoder source contract 与远端 sentinel/边界夹具，没有改写
EventStore、Receipt、Provider、Broker 或外部 exporter 的既有事实路径。它不证明所有输出
通道已经接线，也不提升 durable/live/physical 等级；后续 OA-05/OA-06 负责 sink 与 committed
fact observer 的实际组合。

## 10. OA-04 叠加说明

OA-04 在 `kiana-domain/src/audit.rs` 固定 RuntimeEvent taxonomy，覆盖 command、authorization、
approval、capability、credential、recovery、query 和 export 的稳定 action/decision 映射。未知
非 audit 事件保持 opaque 并跳过；所有 `audit.*` 自报事件（包括 approved/completed/exported）
均拒绝，`audit.correction` 只允许后续以追加事实实现，不能在 reducer 中覆写原记录。

`reduce_audit_records` 要求非零起始 EventCursor、非空 source binding、非零 authority/data epoch、
唯一 source event ID，并为每个来源事件产生 checked cursor、source ID、服务端固定 actor、bounded
Audit redaction 后的 action digest、可选 input/reason/correlation/causation refs 和 record digest。
重复逻辑键、矛盾 decision、伪造 actor/record payload、capability gate 冲突、cursor overflow、
无绑定目标或缺 epoch 均 fail-closed；原始 EventLog 事件不被修改。`kiana-core` 仅暴露同一纯
reducer facade，不访问 Broker、Provider、UI 或 exporter。

这是 domain/core source 与静态编译 proof；OA-04 远端 workflow 承担运行时 taxonomy/reducer 夹具，
本地不执行测试且不声称 durable/live/physical。OA-05/OA-06 仍需建立端口、fake sink、committed
observer 和 projection checkpoint。
