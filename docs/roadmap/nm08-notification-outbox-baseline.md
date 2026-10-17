# NM-08 durable notification outbox and DeliveryWorker baseline

> 快照日期：2026-09-25。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

## Deliverables

- `kiana-domain/src/notification_outbox.rs`：versioned outbox record、lease/fence、attempt
  transition、redaction-safe dispatch intent、receipt/Unknown/reconcile boundary。
- `kiana-ports/src/lib.rs`：enqueue/claim/submit/ack/fail/Unknown 的 outbox port；默认实现
  fail-closed。
- `kiana-eventlog/src/notification_outbox.rs`：`MemoryNotificationOutboxStore` 作为确定性
  source/CI adapter，覆盖单 lease、过期 pre-submit reclaim、stale worker 和 receipt CAS。
- `kiana-core/src/notification_delivery.rs`：`NotificationDeliveryWorker` 只生成
  Claim/Dispatch/AwaitReceipt/Reconcile/Terminal plan，不调用 Broker、channel、provider 或
  `tokio::spawn`。
- GitHub-only fixture/source guard/workflow；roadmap 与 CURRENT_STATUS 证据块。

## Boundary

`feature_status=implemented`; `proof_level=source`。当前 adapter 是进程内 source/CI 语义，
不是 durable/restart/cross-process 存储，也没有外部 channel ACK。lease 过期后的 submitted
attempt 进入 `Unknown`/reconcile；旧 worker、authority epoch、receipt identity/digest、重复
transition 和 direct effect 都 fail-closed。真实 JSONL/SQLite durable outbox、shutdown drain、
OS/Web/connector delivery 和生产 receipt 仍是后续 NM/PD/UAT 范围。

## Completion slice (2026-10-17, CI-only)

上面的 "Deliverables/Boundary" 是 2026-09-25 的窄切片（只覆盖单 lease 与 receipt CAS）。
本次把整张卡片收口为「durable outbox + DeliveryWorker；attempt lease/fence、shutdown drain」：

- **domain**：`NotificationOutboxClass`（best_effort / critical / actionable）与
  `effective_class() = max(declared, required_floor)`；声明低于 notification 已证明的地板
  （带 action ref 的审批类）一律 `notification_outbox_class_understated`，无 action ref 却声称
  actionable 是 `notification_outbox_class_invalid`。新增
  `NotificationOutboxTerminalReason`（delivery_failed / subscription_revoked /
  subscription_expired / subscription_paused / notification_expired）、
  `NotificationOutboxShedReason`、`NotificationOutboxAdmission`、`NotificationOutboxPressure`
  （每个 shed 必须有理由与计数；构造 shed admission 时 protected 记录直接
  `notification_outbox_protected_shed_refused`，账本里 `protected_shed != 0` 同样
  `notification_outbox_protected_shed_observed`）、`NotificationOutboxFenceRefusal`
  （subscription_mismatch / revision / scope / revoked / expired / paused / notification_expired，
  其中撤销/暂停/过期/窗口关闭带 terminal reason）。记录新增 `class`/`terminal_reason` 字段并进入
  digest；`Failed` 必须带理由，其它状态不得带理由。新增 `subscription_fence`/
  `check_subscription_fence` 订阅围栏、`release`（Claimed→Pending，用于 shutdown drain，lease
  token 保留）、`abandon`（带理由终止在途 claim）、`abandon_unclaimed`（终止 pending 或已过期
  claim；拒绝没有投递尝试的 delivery_failed，也拒绝覆盖 submitted）。新增
  `NotificationOutboxTransition` 与 `rebuild_notification_outbox_state`：从
  `pending→claimed→submitted→ack/failed/unknown` 的 committed transition 链重建状态，校验边、
  revision 连续、时钟不回退、每次进入 Claimed 必须推进 lease token（同状态提交=过期 claim 换
  attempt 也在此约束内），失败即 `notification_outbox_rebuild_diverged` 等稳定码。
- **ports**：`NotificationOutboxStore` 新增 `admit_notification`（返回 typed admission）、
  `abandon_notification`、`abandon_unclaimed_notification`、`release_notification_claim`；
  默认实现仍全部 `Unavailable` fail-closed，未实现 bound 的适配器不能假装有界。
- **eventlog adapter**：`MemoryNotificationOutboxStore` 变成有界+可重放适配器：
  `bounded(cap)` 对 best-effort 记录设硬上限，`admit_notification` 在超限或窗口已闭时 shed
  best-effort（`QueueFull`/`NotificationExpired`，进 pressure 账本），critical/actionable 永不
  shed；`enqueue_notification` 对溢出 fail-closed（`notification_outbox_capacity_exceeded`）而不
  静默丢弃。`open_jsonl`/`open_jsonl_bounded` 用 append-only JSONL journal（记录帧+admission 帧，
  0600、权限过宽即 `notification_outbox_journal_permissions_too_broad`、大小/单帧有硬上限）重放：
  新实例得到同样的记录、lease、pressure 计数；篡改 digest、截断首帧、回退 revision 分别以
  `notification_outbox_journal_corrupt` / `truncated_or_unbound_record` fail closed。
  `rebuild_report()` 用 transition 链自证每条记录；每次 commit 先落 journal 再改内存。
- **core worker**：`NotificationDeliveryPlan` 新增 `Abandon { reason }` 与 `ForeignClaim`；
  `plan` 对窗口已闭的记录不再 dispatch，`plan_with_subscription` 先过订阅围栏（撤销/暂停/过期/
  窗口关闭 → 显式 Abandon，submitted 仍走 receipt 路径，他人的 live claim 不碰）；
  `recovery_plan` 让重启后的新进程按同一状态机重建每个记录的 plan（按 outbox_id 排序、重复记录
  拒绝）；`drain` 是有界 shutdown drain：只释放本 worker 未提交的 live claim，受
  `max_releases` 与 `deadline_unix_ms` 双重约束，submitted/pending/过期 claim 分别报告为
  await_receipt/untouched，非终态 protected 记录计数进 `protected_outstanding`；他人 claim
  `notification_outbox_drain_foreign_claim`，epoch 不符
  `notification_outbox_drain_epoch_mismatch`，超期 `notification_outbox_drain_deadline_exceeded`。
- **fixtures**（先拒绝优先）：`kiana-eventlog/tests/nm08_notification_outbox.rs` 12 条（含
  `jsonl_journal_replays_across_restart_and_continues_the_fence`、
  `jsonl_journal_refuses_tampered_truncated_and_permissive_files`、
  `queue_overflow_sheds_only_best_effort_with_reason_and_keeps_protected_records`、
  `understated_class_and_protected_shed_are_refused`、
  `lifecycle_is_rebuildable_from_the_transition_chain`、
  `revoked_subscription_cannot_claim_dispatch_or_acknowledge`、
  `durable_store_never_duplicates_a_dispatch_after_repeated_replays`）；
  `kiana-core/tests/nm08_notification_delivery.rs` 8 条（含
  `worker_drain_is_bounded_and_reports_outstanding_protected`、
  `worker_recovery_plan_restores_each_lifecycle_state_deterministically`、
  `worker_plans_a_restored_jsonl_outbox_without_duplicate_dispatch`）。
  两条 target 都在自动 shard（kiana-eventlog 整 crate、`kiana-core-s4/6`）里，另有手动
  `.github/workflows/nm08-notification-outbox.yml`（`workflow_dispatch`-only、`contents: read`、
  fmt 门 + 三个 target 的 run/`--no-run`）。
- 无 `Cargo.toml`/`Cargo.lock`/`package.json` 变化、无新依赖/新执行循环；五个模型可见工具面不变。

如果验收测试名与卡片不同，以本文件列出的夹具名为准（卡片只给了验收语义）。

## Known limits

- proof_level 仍是 `source`：本地未运行任何测试；即使 CI 绿，也只是 in-process + 单写者 JSONL
  journal 的确定性回执，不是跨进程事务、不是 fsync-per-frame WAL、不是生产 SQLite/EventLog。
- journal 是单写者语义：多进程同开同一文件、崩溃在 journal append 与内存提交之间的窗口、磁盘满
  都未做端到端证明（append 失败会返回错误且内存不变，但恢复语义留待 fault injection）。
- 真实 channel/OS/Web/connector 投递、provider receipt、dead-letter 与 reconciliation 闭环仍属
  NM-18/NM-19/NM-20；本步骤只到「typed plan + 本地 receipt」。
- `protected_outstanding` 只说“还有非终态 protected 记录”，不代表告警/升级路径已接线
  （NM-11/NM-17）。
