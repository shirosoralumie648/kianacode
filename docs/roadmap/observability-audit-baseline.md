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
| Schema/ID registry | `kiana-domain/src/contracts.rs` | `0b6c395976b521cf189af498fcda6a7baf7ce9fcd6a3cd649ae72c1143299a40`（OA-05 health schema 扩展） |
| Domain exports | `kiana-domain/src/lib.rs` | `ac35afaebdb152eddb0898f7e9a28a886d17abdc35643d71cdfb2267e3f731a4`（OA-04 audit reducer 导出） |
| Audit taxonomy/reducer（OA-04） | `kiana-domain/src/audit.rs` | `b17e84def5825be9c2fbdfa704ffe12f9502821325e0c2f66e977b1d72aed1e1` |
| Core audit facade（OA-04） | `kiana-core/src/audit.rs` | `9c444ff6a125a90ac6ba25c1756802e826c40e681ce24f21645f1f08eff9ee22` |
| Health snapshot（OA-05） | `kiana-domain/src/observability.rs` | `03b6f822d5bac02c4717e2cf32796f6e8bfe53e853670b25ae88c0e98d5c7ea2` |
| Span lifecycle contract（OA-07 overlay） | `kiana-domain/src/observability.rs` | `2079d5fcc4850d50e9e73a1f386144d81c03c1d4d8ccb7936062ce4e739e0d5f` |
| Model attempt contract（OA-08 overlay） | `kiana-domain/src/observability.rs` | `5e53b7a30fc87243e4c997f2dd9a9f2fb5108e7cfe6eb67ddda0aa40d7deceeb` |
| Capability attempt contract（OA-09 overlay） | `kiana-domain/src/observability.rs` | `80d4c60dfd20b9508b39c82c2ad39fdb9b7fbd16a45b3854ec27e5ad8903d708` |
| Signal ports/fakes/observer contract（OA-05/OA-06） | `kiana-ports/src/lib.rs` | `36fca0363cd1aab7ca00d3a75375be99ab6aeb13de28f38e42fcf425a96b16f0` |
| Correlation links（OA-02 overlay） | `kiana-domain/src/correlation.rs` | `6fabe5e7cb8d2c86604738108eebcea6274d36306afac197a6115d13b1bfa951` |
| Event construction/redaction | `kiana-core/src/events.rs` | `7d1852ad4a9d93792256288a01a25e06c677e0f6641274b2575b718c87020679` |
| Receipt projection | `kiana-core/src/receipts.rs` | `dda33c346b1ffd94389093e2856f99433ea083a3c61b35cf562484f9f4bc7e1e` |
| Run/invocation projection | `kiana-core/src/projection.rs` | `20d84eb8fa0ac77ac85b48acb10e2fcc78214cc1c3c10be339b540230b1ff68` |
| Recovery/checkpoint | `kiana-core/src/recovery.rs` | `f964c69bb32e940782ef52b399afd9ad6902778b4900f6b7164cb79f3dcc7d26` |
| Lifecycle receipt write | `kiana-core/src/lifecycle.rs` | `346ecb8ea2879b24a53a2ee238bb46c3f7793ffa6211c126ec86ad49c47bc237` |
| Golden trace/replay | `kiana-core/src/versioning.rs` | `679c88757469189f93b20163bbfccb9d65a161ab24ec9f6d9e73a483586108ab` |
| Review/Outcome boundary | `kiana-core/src/collaboration.rs` | `49e61cf89add855ca4fc3687104d666388477848d63397b9ea50b0404021dabf` |
| EventStore facade | `kiana-eventlog/src/lib.rs` | `7af0aba67a44e4575ffd7920ab07866a28c4dbbfcf0c7a3a96221986dfc190bb`（OA-06 observer export） |
| Commit observer wrapper（OA-06） | `kiana-eventlog/src/stream.rs` | `bc955e9c583466a97c1fb02fda4bff8af076dd04ee8df342e21777345b9c033a` |
| Span lifecycle projection（OA-07 overlay） | `kiana-core/src/span_projection.rs` | `e28d545faeb51c8ff1c6410aac0f8b96687a8e283641cb6ac6887db15b9fdfec` |
| Model attempt projection（OA-08 overlay） | `kiana-core/src/model_attempt_projection.rs` | `ce723db86729bbce0c9391157dbf8140bb09470a8d81ba05ebf4ba613a168de0` |
| Capability attempt projection（OA-09 overlay） | `kiana-core/src/capability_attempt_projection.rs` | `e2de85b4baaabdf0afd9b7b60713e96b950a771f6354008b9daec4791924986a` |
| Core capability-attempt exports（OA-09 overlay） | `kiana-core/src/lib.rs` | `ab12f84c7cfa46ab7744801364608a7d8e98994d0a70d78b12544f40e67bdd87` |
| Capability admission/effect instrumentation（OA-09 overlay） | `kiana-core/src/capabilities.rs` | `34a78eb181ef97253ab41d254ca298e621e9ea540ccec73a0bb94fde7dfbd061` |
| Approval effect metadata（OA-09 overlay） | `kiana-core/src/approvals.rs` | `439a602ee0463d1be33ce144d787e5b7b6cbdb19db7277de2da248aa0cf28b1e` |
| Dispatch permit/execution boundary（OA-09 overlay） | `kiana-core/src/dispatch.rs` | `75ef963c49c89cc3848a542f3dc5a63a01e55d2321b47b1f4bb918addd22f672` |
| Capability event metadata（OA-09 overlay） | `kiana-core/src/events.rs` | `5a0348f5e1940363119d920244724428af1e1373f692f6424ccfd3b8a6dfe26e` |
| Harness effect/stop metadata（OA-09 overlay） | `kiana-daemon/src/harness_capabilities.rs` | `342dcbca27709f41821872c60ab199595f51ebc6fa9dccabbaf0b040ae5c46ce` |
| Provider safe telemetry（OA-08 overlay） | `kiana-provider/src/telemetry.rs` | `1a9ec235b5d0cf8c420d758bde425ff89a0ea8c7d746c38f09daf6a7dfc835d5` |
| Daemon model boundary（OA-08 overlay） | `kiana-daemon/src/model_client.rs` | `02bf42ff2171842679421621d128be26d44298457d758850499eae1694cfa0b9` |
| EventStore append planning | `kiana-eventlog/src/event_store_core.rs` | `506a8222a757a89e6516a12b6260daa7f35af2b3da49c17a23ca7cdbcc9d0b1e` |
| Journal state/replay | `kiana-eventlog/src/journal_core.rs` | `84ef64bc8208b2ead45d1d05feb0265e94129b898cd67d04f5ed189d388bba56` |
| JSONL adapter | `kiana-eventlog/src/jsonl.rs` | `f549e5de5bc4e197f268b865327bd1d0d7a4c10208daa3e7c13ceb68188e5c5f` |
| Memory adapter | `kiana-eventlog/src/memory.rs` | `bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9` |
| RunStream projection | `kiana-daemon/src/run_stream.rs` | `750bb78e46e5f1c42d3a773d0afcf468cc6713032ef47e30ffa100f3313db5c6` |
| Daemon composition | `kiana-daemon/src/lib.rs` | `f262e808ab239eed0e01c1e28aef75cb5d78b819d809c1d07592ab34d367d281` |
| Port contract | `kiana-ports/src/lib.rs` | `36fca0363cd1aab7ca00d3a75375be99ab6aeb13de28f38e42fcf425a96b16f0`（OA-02 correlation + OA-05 signal + OA-06 observer overlay） |

## 2. Signal matrix

| 信号/对象 | 当前 owner 与接线 | 事实还是派生 | 当前限制 / 迁移 owner |
|---|---|---|---|
| `RuntimeEvent` | `kiana-domain::RuntimeEvent` 是 `kind: String + data: Value`，由 `ControlPlane::append_event`/`record_terminal_event` 构造 | EventLog 中的事实载荷；不是完整 signal schema | 没有 event-kind registry、schema owner、timestamp、correlation/causation 或 DataClass；OA-01/OA-02 |
| `TransitionBatch` / `CommandReceipt` / `CommitOutcome` | domain journal 合同 + `EventStorePort`；JSONL/Memory journal 做 CAS、幂等和 cursor；OA-06 `StreamEventStore` 只在 Committed 后通知 | 命令提交事实 | receipt 证明提交边界，不证明 dispatch/effect/业务 Outcome；observer 是 wake/diagnostic hint，重启仍须按 cursor 扫描；EventStore port 本身不承诺 durable；ER-02/OA-10 |
| `MemoryEventLog` | `kiana-eventlog::MemoryEventLog` | 进程内事实缓存 | `durable_commits=false`，不能作为 durable observability evidence；ER/PD |
| `JsonlEventLog` | JSONL frame、锁、checksum/torn-tail/Unknown 路径 | 本地 EventLog 事实 adapter | `cfg!(unix)` 能力和源码不等于重启/物理 durability 证明；没有 observability projector；ER-31/PD-27 |
| Run / Invocation projection | `kiana-core::projection` 从 run/approval/capability 事件折叠，内存 map 仅缓存 | 派生状态 | 缺正式 projection version、source cursor、lag/checkpoint；缓存不是事实；OA-06/OA-10 |
| `CommandReceipt` → Run receipt | `kiana-core::receipts::receipt_from_events` 从过滤后的 EventLog 生成，并最终再做 redaction | 派生 Receipt | 缺顶层 `source_cursor`/`source_event_ids`/`projection_version`/`limitations` 合同；Receipt ≠ business Outcome；ER-01/ER-30 |
| `run.receipt` event | lifecycle 在成功路径把派生 receipt 再写入 EventLog | 现有事件事实中的 shadow copy | 易使 receipt 看似第二事实源；必须区分原始事实与可重算 projection；OA-06/ER-02 |
| `ResultUnknown` | receipt/projection 对缺 terminal、矛盾 terminal、读取/投影异常保留 `ResultUnknown` | 事实驱动的失败分类 | 没有统一 Incident/Recovery/Audit projection 与 operator query；ER-30/OA-10/OA-19 |
| RunStream / SSE / Workbench | `RunStreamBus` + daemon `StreamEventStore`；只在 committed fresh append 投影，replay 不重复；OA-06 eventlog wrapper 提供通用 committed observer | 进程内、bounded、best-effort 展示 | broadcast send 可丢、epoch/cursor 在内存中；gap/terminal replay 不是 EventLog 事实、授权或 Outcome；OA-13/OA-24 |
| transcript / UI timeline / cache | 入口与 `kiana-screens`/context index 读取 RunStream 或缓存 | 派生视图/加速缓存 | 不能作为状态、审计、成功、健康或恢复依据；OA-16/OA-20/OA-24 |
| model turn / usage | `run.model_turn` 事件携带 provider/model/route/prompt hash、stream/usage/elapsed/finish/retry；OA-08 的 `kiana-core::model_attempt_projection` 从 committed 事件派生 `ModelAttemptRecord`，`UsageRecord` 仍负责成本账本 | 事件中局部事实 + 有界模型 attempt 投影 | prompt/header/raw response 不可进入 projection；malformed/truncated/timeout/retry/missing usage 不得为 `ok`；`run.usage`、`model.usage`、`provider.usage`、`usage.recorded` 的命名收敛与 MetricCatalog/单位/低基数 labels 仍由 OA-10/OA-12 完成 |
| Business Metric / Incident | Company closeout 的 `MetricObservation`、`Incident` 和 platform `FailureIncident` | Company/故障领域事实，不是 OA signal contract | 不能把业务指标或 Incident DTO 当 runtime MetricSnapshot/HealthSnapshot/AuditRecord；OA-04/OA-11/OA-19 |
| `PreparedModelCall::audit()` | provider/model 请求的安全摘要 | 局部诊断摘要 | 不是 `AuditRecord`，无 source event/cursor、decision taxonomy、retention/query boundary；OA-03/OA-04 |
| `golden_trace.captured` / `trace.replay` | `kiana-core::versioning` 复制 run events/receipt，校验 owner/project/hash/revocation，replay 明确 `side_effects=false`/`provider_calls=0` | EventLog 中的评测快照与只读 replay | golden trace ≠ telemetry trace；trace 不得授权、恢复或替代 event order；完整 events/receipt shadow copy 的 retention 需迁移；OA-02/OA-14/OA-21 |
| Operational log | 没有 `ObservabilityPort`/结构化 log sink；OA-03 提供 `RedactionProfile`/bounded encoder | profile/encoder source 已实现；runtime sink 未实现 | exporter、脱敏失败、队列、flush/关闭 ack 未建模；OA-05/OA-13 |
| Metric | OA-01 已注册 `MetricCatalog`/`MetricPoint` domain contract，OA-03 提供 Metric profile boundary；仍没有 reducer/sink | schema/encoder source 已实现；runtime 未实现 | 不得把 usage counter、UI cursor 或 Company metric 当 canonical runtime metrics；OA-10/OA-12 |
| Trace / span | OA-01 已注册 `TraceSummary`，OA-02 已注册 `CorrelationContext`/`TraceRef`/`SpanRef` 与 link，OA-03 提供 Trace bounded encoder；OA-07 的 `kiana-core::span_projection` 派生 Run/Turn/Invocation lifecycle，OA-08 的 `ModelAttemptRecord` 以稳定 model span ID 关联 provider attempt；没有 exporter/`TraceSink` runtime 接线 | schema/correlation/encoder/projection source 已实现；runtime exporter 未实现 | trace 只可关联，不可作为 actor/authority/policy/approval；span/attempt end 不制造 terminal，迟到/重复/旧 attempt 不覆盖事实；OA-09/OA-14 |
| Audit | OA-04 在 `kiana-domain/src/audit.rs` 固定 taxonomy，并由 `kiana-core/src/audit.rs` 从带 cursor/source binding 的 committed `RuntimeEvent` 派生 `AuditRecord`；OA-06 `StreamEventStore` 只在新 Committed 后通知，OA-05 `AuditQueryPort` 仅读投影 | taxonomy/reducer、observer/query port source 已实现；checkpoint projector 未实现 | 模型/UI/plugin/exporter 不能伪造批准/完成/导出；未知 `audit.*`、缺 source/epoch、矛盾 decision、重复 source 记录 fail-closed；observer 丢失须 cursor 扫描；OA-10/OA-15/OA-16 |
| Health / Incident signal | OA-05 新增 versioned `HealthSnapshot` 与 `HealthProbePort`；Health 仍是 probe projection，不是授权或业务 Outcome | snapshot/port/fake source 已实现；runtime probe、lag/incident projector 未实现 | stale、projector gap、unknown exporter、journal corruption 不能返回 healthy；OA-10/OA-11/OA-19 |

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
| OA-05 | Observability/Trace/Metric/Audit/Health ports 与 fake adapters | `source`；ports、Memory/JSONL fake、flush/cancel/capacity/query/probe contracts 已实现，尚无 EventLog observer 或 durable sink |
| OA-06 | commit observer 只通知 Committed，重放不重复通知，建立 projection cursor | `source`；`kiana-ports` 的 `CommittedTransition`/observer contract 与 `kiana-eventlog::StreamEventStore` 已实现；通知是可丢 wake hint，尚无 durable checkpoint |
| OA-07 | Run/Turn/Invocation span 生命周期与 runner/event projection bridge | `source`；`SpanLifecycleRecord`、稳定 trace/span ID、只读 reducer 和 ControlPlane bridge 已实现；尚无 exporter、durable checkpoint 或 live backend |
| OA-08 | provider/model/stream/usage instrumentation | `source`；`ModelAttemptRecord`、provider safe prepared summary、daemon model-port boundary 和 committed-event reducer 已实现；尚无 MetricSink/TraceSink exporter、durable checkpoint 或 live backend |
| OA-08 | provider/model/stream/usage instrumentation | `source`；`ModelAttemptRecord`、safe prepared summary 与 committed-event reducer 已实现；尚无 MetricSink/TraceSink exporter、durable checkpoint 或 live backend |
| OA-09 | broker/approval/effect/stop instrumentation | `source`；`CapabilityAttemptRecord`、handler 前 execution CAS、拒绝/过期/TOCTOU/取消 stop evidence 已实现；尚无 durable attempt checkpoint、外部 effect receipt、reconcile projector 或 live exporter |
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
通道已经接线，也不提升 durable/live/physical 等级；OA-05/OA-06 已分别固定 sink 端口/fake
adapter 与 committed fact observer，后续 OA-07+ 负责 runtime producer/projector 接线。

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
本地不执行测试且不声称 durable/live/physical。OA-05/OA-06 已建立端口、fake sink 和 committed
observer；projection checkpoint 留给 OA-10/OA-15。

## 11. OA-05 叠加说明

OA-05 在 `kiana-ports` 增加 `ObservabilityPort`、`TraceSink`、`MetricSink`、`AuditQueryPort` 和
`HealthProbePort`，以 `ObservabilitySignalRecord` 封闭 signal union，统一返回 append/flush ack，
并提供 durable/flush/cancellation/capacity capability negotiation。`AuditQueryRequest`/`Page`
固定 source cursor、bounded page/filter、projection version 和空页/不可用错误边界；query 不暴露
raw RuntimeEvent。`HealthSnapshot` 在 domain 注册 `kiana.health-snapshot.v1`，绑定 cursor/source
IDs、status、observed time、bounded capabilities/limitations 与 digest。

`MemoryObservabilitySink` 与 `JsonlObservabilitySink` 是显式 non-durable fake：可记录 log/metric/
trace/audit/health、注入一次性失败、容量拒绝、取消和 flush ack，且仅返回已验证 projection；
不依赖或调用 Broker，不把 exporter/sink ack 变成授权或效果事实。缺 durable/flush/cancel 或容量
能力的 adapter 由 `require_observability_capabilities` fail-closed。OA-05 远端 workflow 承担运行时
fake/query/probe 夹具，本地不执行测试；OA-06 已补 EventLog commit observer，真实 durable sink、
backpressure 和入口接线留给 OA-13/OA-24。

## 12. OA-06 叠加说明

OA-06 在 `kiana-ports` 固定 `CommittedTransition` 与 `EventStoreCommitObserver`：通知携带原始
`TransitionBatch`、同一 `CommandReceipt`、连续 `first_cursor/cursor` 和严格对应的
`source_event_ids`，伪造 receipt、cursor 回退、事件 ID 错配或重复 ID 均 fail-closed。通知
只描述新提交，不授予权限，也不调用 Broker。

`kiana-eventlog::StreamEventStore` 是现有 EventStore 的装饰器；仅当内层返回
`CommitOutcome::Committed` 时按注册顺序调用 observer，`Replayed`、`Conflict`、`Unknown` 和
底层错误都不发布。observer 失败只进入有界 `CommitObserverFailure` 诊断队列，已提交结果不
被改写成假拒绝，也不会因重试重放再次通知。`read_from` 仍是重启后的事实补偿路径，因此
callback 丢失不等于 projection 已同步。legacy `append*` 兼容路径不伪造 transition receipt，
继续由既有 adapter 语义负责。

OA-06 远端 workflow 覆盖 fresh commit、replay、CAS conflict、Unknown、observer failure 和
receipt/cursor 伪造夹具；本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制，
不宣称 durable/live/physical。

## 13. OA-07 叠加说明

OA-07 在 `kiana-domain` 注册 `kiana.span-lifecycle.v1`，并以 `SpanLifecycleRecord` 固定
Run、Turn、Invocation 三类 span 的稳定 `TraceId`/`SpanId`、实体关联、attempt、生命周期阶段、
`TraceStatus`、来源 cursor/event、受限错误码和低基数属性。记录要求单一 source event、非零
cursor、严格的实体 ID 关系、bounded attributes 与 digest；它是可重算 projection，不是授权
令牌，也不创建或修改 EventLog 事实。

`kiana-core::span_projection::project_span_lifecycle` 按输入事实顺序折叠 `run.authorized` /
`run.prompt` / `run.started`、approval、invocation dispatch/result、compact、cancel 和 terminal
事件。稳定 trace/span ID 从 run/实体键的 digest 派生，重复 event ID 和重复 terminal 幂等；
terminal 冲突 fail-closed，迟到 delta/approval/result、旧 attempt 不覆盖已结束 span，新 attempt
只由更高 attempt 明确重开。没有可验证 invocation/turn 关联的事件保持 opaque，不猜造 span。
`ControlPlane::span_lifecycle`/`span_state` 只读 EventLog 重建入口，不访问 Runner、Broker 或
Exporter，也不会把 span end 当成 run terminal。

OA-07 远端 workflow 覆盖 deterministic replay、Run/Turn/Invocation start/pause/resume/checkpoint/
end、cancel/unknown、duplicate terminal、late delta、terminal conflict 和 stale attempt 夹具；
本地只执行格式、静态源码检查和 test-target 编译，不执行测试二进制，不宣称 durable/live/physical。

## 14. OA-08 叠加说明

OA-08 在 `kiana-domain` 注册 `kiana.model-attempt.v1`，以 `ModelAttemptRecord` 固定 provider、
model、route digest、prompt version hash、streaming、latency、stop reason、usage、retry class 和
低基数 cache usage。记录带稳定 trace/span ID、model call/request/attempt、source cursor/event 和
canonical digest；`status=ok` 只有在 attempted、完整 stop (`end_turn`/`tool_use`)、完整 usage、无
错误且 retry class 为 `never` 时成立。缺字段、未知 stop、超界 usage、截断/超时/连接失败、重试或
未完成 usage 一律保留 `unknown`/`degraded`，不能伪造成功。

`kiana-provider::safe_prepared_metadata` 和 daemon 的 `InstrumentedModelClient` 共同执行
allow-list：只暴露 route identity 的 digest、request/attempt ID、provider/model、prompt/request hash、
budget 和 streaming；compiled wire body、prompt/messages、工具参数、endpoint、鉴权 header、cache key
和 raw provider response 不可进入该摘要。Harness 仍只通过现有 `run.model_turn` 事件落账，
`kiana-core::model_attempt_projection` 从 committed EventLog 重建记录并提供只读
`ControlPlane::model_attempts`/`provider_attempts`，不创建第二模型循环或 sink。

OA-08 远端 workflow 覆盖正常流、secret sentinel、malformed/truncated/timeout/retry、缺 usage、
cache/stop/retry 分类、重复 attempt 和 contract fail-closed 夹具；provider 测试只构造离线
prepared 请求。此次本地仅执行格式、静态源码检查和 test-target 编译，不执行测试二进制；该投影
仍是 source proof，不等价于 Receipt、账单、Metric/Audit 对账、durable/live telemetry 或外部 provider
结果。

## 15. OA-09 叠加说明

OA-09 在 `kiana-domain` 注册 `kiana.capability-attempt.v1`，以
`CapabilityAttemptRecord` 固定一次能力请求的 admission、approval、permit、dispatch、execution、
effect、stop、fencing 和 zero-effect 证据。记录只保留稳定 ID、operation/action digest、低基数状态、
source cursor/event 与有界错误码；原始参数、shell command、路径、header、secret 和 handler 输出不
可表示。`status=ok` 必须同时满足 committed allowed admission、已知成功 effect、无错误，且不能是
zero-effect；`effect=unknown` 或 stop 未确认时始终保持 `fenced=true`。

`kiana-core::capability_attempt_projection` 只消费已提交 request/decision/approval/permit/
dispatch/execution/result/cancel facts，按 `(request_id, attempt)` 稳定折叠，并通过
`ControlPlane::capability_attempts`/`effect_attempts` 提供只读重建入口。ControlPlane 在调用 Broker
handler 前以 execution-permit CAS 追加 `invocation.executing`，因此 execution boundary 未提交时不
调用 handler；拒绝、hook/policy block、过期 approval、lease/authority/TOCTOU mismatch 均保留
`zero_effect` 或 `unknown`，不会被 telemetry、cancel 请求或 UI 结果覆盖。daemon shell/patch handler
仅补充 bounded effect/stop metadata，仍沿原有 Broker 主链执行。

OA-09 远端 workflow 覆盖成功 admission→permit→dispatch→execution→result、policy/hook deny、过期
审批、TOCTOU/lease unknown、cancel 未确认和 secret sentinel；本地仅执行格式、静态源码检查和
test-target 编译，不执行测试二进制。该记录是 EventLog 派生 source proof，不等价于外部效果 receipt、
durable checkpoint、reconcile 完成或 live stop/telemetry 证明。
