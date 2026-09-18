# NM-04 Notification Projector Baseline

## Scope

NM-04 adds a committed-only `NotificationProjection` over the existing replay/checkpoint helper.
Only registered `NotificationEventRegistry` facts declared from `EventLog` are materialized;
model/UI sources, unknown notification families, missing payloads and invalid notification DTOs
fail without mutating the prior projection. The projection stores source cursor/event IDs through a
digest-bound checkpoint and reapplying the same event is idempotent.

Recipient/subscription scope resolution, notification dedup/OCC, durable inbox/outbox and delivery
workers remain NM-05+; this projector never sends or authorizes anything.

## Evidence and limits

- `kiana-core/tests/nm04_notification_projector.rs` covers committed event materialization,
  checkpoint cursor, replay idempotency, untrusted source denial and cursor-gap no-mutation.
- `kiana-core/tests/nm04_notification_guard.rs` protects committed-only/checkpoint/no-delivery
  boundaries. GitHub Actions runs fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable NotificationStore,
recipient resolver, subscription revision, outbox or external delivery proof is claimed.
