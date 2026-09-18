# BQ-08 Quota Reservation Baseline

## Scope

BQ-08 adds digest-bound `QuotaReservation` identity/state with idempotency key, authority epoch,
config revision, owner run/attempt, quota group/window, revision and requested dimensions. State
transitions are explicit and unknown can only be reconciled to settled/released. The
`QuotaReservationPort` and in-memory fixture adapter implement idempotent replay, digest/CAS
conflict rejection and authority/config fence checks before transitions.

The adapter is deliberately marked in-memory: it is a conformance fixture, not durable EventLog or
cross-process proof. A later durable adapter must preserve the same contract and add atomic CAS,
flush/recovery and two-writer evidence; no provider dispatch occurs here.

## Evidence and limits

- `kiana-domain/tests/bq08_quota_reservation.rs` covers digest/state/revision/expiry validation.
- `kiana-ports/tests/bq08_quota_reservation_store.rs` covers replay, digest conflict, stale
  revision and authority/config fence denial.
- `kiana-core/tests/bq08_quota_reservation_guard.rs` protects the contract/port boundary.
  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=partial`, `proof_level=source`: the in-memory adapter is not durable,
and no EventLog-backed reservation, cross-process race, permit, queue or external billing effect is
claimed. BQ-08 durable integration remains open.
