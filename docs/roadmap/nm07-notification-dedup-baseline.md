# NM-07 Notification Deduplication Baseline

## Scope

NM-07 adds a typed notification-intent deduplication contract. A deduplication key is bound to
the notification content digest, subscription revision and a monotonic record revision. The
in-process eventlog adapter performs claim and compare-and-swap under one mutex: at-least-once
input folds to one accepted intent, an equal retry returns the original notification, a changed
digest or stale revision fails closed, and concurrent updates have one winner.

The adapter ends at the semantic notification-intent boundary. It does not create an outbox item,
claim a delivery attempt, call a channel, invoke a Broker or provide durable/restart/cross-process
evidence; those remain later steps.

## Evidence and limits

- `kiana-eventlog/tests/nm07_notification_dedup.rs` covers one-winner claim/replay, same-key
  digest conflict, stale revision and concurrent CAS.
- `kiana-core/tests/nm07_notification_dedup_guard.rs` protects the typed OCC/no-dispatch boundary.
- `.github/workflows/nm07-notification-dedup.yml` runs the fixtures, source guard and workspace
  compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the source contract and CI
fixtures are present, while durable persistence, restart recovery, multi-process CAS and channel
delivery are not claimed.
