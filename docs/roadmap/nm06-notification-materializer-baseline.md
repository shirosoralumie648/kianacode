# NM-06 Notification Materializer Baseline

## Scope

NM-06 adds a bounded in-memory `NotificationMaterializer` over committed EventLog facts.  It
requires the registered event source, an exact source event ID and cursor, redacted summary,
evidence references, action references and due/expiry window.  A `HumanTaskBridge` binds the
immutable task target and decider while deliberately omitting task status; the original
HumanTask/approval/acceptance object remains the status authority.  The materializer returns the
existing display-only `HumanInboxItem` after exact server-recipient and decider filtering and a
stable sort.

The projection rejects cursor gaps, source-event/digest conflicts, missing payload/evidence/due
metadata and untrusted event sources.  Reapplying the same event and digest at the same cursor is
idempotent.  It does not own a `NotificationStore`, outbox, delivery worker, provider, Broker or
second execution loop.

## Evidence and limits

- `kiana-core/tests/nm06_notification_materializer.rs` covers committed source validation,
  missing source/evidence/due data, cursor gaps, replay/conflict behavior, redacted summaries,
  recipient/decider filtering and display projection.
- `kiana-core/tests/nm06_notification_materializer_guard.rs` protects the projection-only,
  server-scoped boundary. GitHub Actions runs fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable materializer,
HumanTask store, notification outbox, delivery worker, external channel or live/physical proof is
claimed.
