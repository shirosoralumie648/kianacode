# INT-21 connector retry baseline

## Scope

INT-21 defines a connector-specific retry classifier and bounded attempt policy. A retry is a
decision for a new attempt, never a permission to reuse a consumed permit or replay an unknown
effect. The classifier admits only known-no-effect or explicitly declared-idempotent observations
with an idempotency key, within absolute deadline, attempt and backoff limits.

## Implemented source slice

- `ConnectorRetryObservation` accepts only bounded, digest-bound, redacted classification evidence.
  Unknown, approval-denied, epoch-stale, scope-denied, validation-failed and cancelled classes are
  explicit deny outcomes.
- `ConnectorRetryPolicy` enforces attempt/deadline/backoff bounds. `KnownNoEffect` requires no
  request sent and `NoEffect`; `DeclaredIdempotent` additionally requires a provider-declared
  idempotency key and still rejects an effect state that is not `NoEffect`.
- `ConnectorRetryDecision` binds the policy digest and next attempt/delay. It does not call a
  Broker, EventStore, adapter or scheduler; the caller must perform fresh INT-16/INT-18 admission.

## CI-only evidence

GitHub Actions runs connector retry fixtures, the pure source guard, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: partial
proof_level: source
```

This slice classifies supplied evidence only. It does not schedule retries, create new durable
reservations, prove provider no-effect truth, implement timeout cancellation, reconcile Unknown,
or demonstrate live/physical connector behavior. Those remain INT-22/INT-23 and later integration
work.
