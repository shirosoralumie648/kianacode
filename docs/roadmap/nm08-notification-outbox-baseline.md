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
