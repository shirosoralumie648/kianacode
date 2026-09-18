# NM-05 Notification Resolver Baseline

## Scope

NM-05 adds a pure server-context notification resolver. It takes an authenticated `RequestContext`,
server-derived project ID, typed Notification and subscription snapshot; it requires trusted
project/actor, exact recipient/project binding, active unexpired subscription and notification
scope/channel subset. Client-supplied owner/project/recipient values are never authority inputs.

The resolver returns only eligible subscription snapshots. It does not send, claim, acknowledge,
mutate read state or call Broker/Provider; delivery and OCC remain NM-06+.

## Evidence and limits

- `kiana-core/tests/nm05_notification_resolver.rs` covers trusted success, untrusted project,
  recipient mismatch and server project/scope binding.
- `kiana-core/tests/nm05_notification_resolver_guard.rs` protects the no-dispatch/server-context
  boundary. GitHub Actions runs fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable subscription store,
authority/assignment resolver, notification dedup/OCC, inbox/outbox or external delivery proof is
claimed.
