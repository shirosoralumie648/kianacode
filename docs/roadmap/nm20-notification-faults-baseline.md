# NM-20 notification fault, capacity and security matrix baseline

> Snapshot date: 2026-09-26; completion slice 2026-10-17. Tests are GitHub Actions-only; this step
> does not run local tests, build, check, clippy or smoke commands and does not wait for CI.

`NotificationFaultMatrix` is a deterministic replay-only classification over nine notification
failure scenarios: duplicate, out-of-order, cursor gap, crash-after-claim, slow consumer, queue
full, disk full, secret sentinel and projection loss. Each case binds source cursor/event IDs and
records a bounded disposition, critical-fact preservation, duplicate suppression, snapshot/reconcile
requirements, `effect_started=false` and `secret_free=true` invariants.

The matrix does not inject a crash, fill a disk, drop a queue item, write EventLog, call a provider,
or send a notification. It is a safety contract to drive later EventLog/Projector/Worker/Channel/UI
fault tests; critical/terminal/approval facts are never silently downgraded to successful empty
state.

`feature_status=implemented`; `proof_level=source`.

Known limits: no real kill-9/power-loss/disk-full/network/slow-browser/provider/connector test,
durable checkpoint, capacity benchmark or live/physical receipt evidence is claimed.

## Completion slice (2026-10-17, CI-only)

The 2026-09-26 slice was a replay-only classification; it never drove a seam. This slice adds the
driving harness `kiana-core/src/notification_fault_harness.rs`
(`kiana.nm20-fault-harness-run.v1`): `Nm20FaultCase` carries twelve injections over the nine
families, every case drives the seam it names through an injected `Nm20FaultSeam`, and
`Nm20FaultHarnessRun::{evaluate_with_seam, from_observations, missing_cases}` reports honestly which
cases a target did not cover instead of pretending full coverage. `Nm20LibrarySeam` refuses the
four adapter-backed cases with `nm20_adapter_seam_required:<case>`.

Library cases: duplicate replay (`notification_projection_duplicate_suppressed`, one row, stable
checkpoint), stale-cursor regression (`projection_checkpoint_cursor_not_advanced`, projection
unchanged), cursor gap (`projection_checkpoint_cursor_gap`, snapshot required),
lease reclaim fence (live lease not stealable; reclaim bumps attempt/generation; the stale lease
gets `notification_outbox_lease_fence_mismatch` then `notification_outbox_lease_expired`),
disk full (`platform_disk_full`, never over-matched; `NotificationDeliveryWorker::plan` only ever
moves Submitted to ReconcileUnknown/Unknown/Terminal), secret sentinel
(`secret_sentinel_quarantined`, redacted form passes), projection loss
(`notification_projection_rebuild_equivalent`), critical query fallback
(`notification_query_fallback_served`: the committed inbox query still answers the actionable
approval row; an unadvanced projection answers `ProjectionUnavailable`, never an empty inbox).

Adapter cases drive the real seams: `kiana-eventlog` `MemoryNotificationOutboxStore` (crash after
claim reclaims only through lease expiry and fences the stale lease; bounded admission sheds
best-effort with `notification_outbox_queue_full_shed_counted`, never a protected record, and
refuses overflow with `notification_outbox_capacity_exceeded`) and `kiana-daemon`
`NotificationStreamBridge` (an unhydrated delta becomes SnapshotRequired, a merge-window overflow
clears the buffer without a partial merge; one subscriber's gap leaves the other's cursor/terminal
untouched and demands the authoritative query).

Fixtures: `kiana-core/tests/nm20_notification_fault_harness.rs` (13 tests: one named test per card
item, the library seam refusing adapter cases, run shape and fixture contract),
`nm20_notification_fault_harness_guard.rs` (2 guards: nine-family/twelve-case/eleven-refusal
matrix, eleven forbidden effect strings, shard and lane binding),
`kiana-daemon/tests/nm20_notification_channel_faults.rs` plus
`nm20_notification_channel_faults_guard.rs`. The two targets
compose to the full twelve-case coverage through honest `missing_cases` reports. The new core
targets are claimed by the existing `kiana-core-s4/6` shard; the daemon whole-crate shard discovers
its targets automatically. The rebuilt manual lane `.github/workflows/nm20-notification-faults.yml`
runs the classification matrix and both driving targets in both crates (`--test-threads=1`) plus
`--no-run` compiles, `workflow_dispatch`-only with `contents: read`.

Known limits: still no real kill-9/power-loss/ENOSPC/network/slow-browser test; the crash window is
the in-memory outbox store's reclaim semantics, the disk full is the classifier plus the worker
plan, and subscriber isolation is a single-process bridge. No durable checkpoint, capacity
benchmark, cross-process subscriber recovery or live/physical receipt evidence is claimed.
