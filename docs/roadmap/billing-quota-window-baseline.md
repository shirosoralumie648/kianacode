# BQ-07 Quota Window Baseline

## Scope

BQ-07 adds trusted-clock UTC quota windows, canonical provider/credential/model/alias group keys,
and bounded window-budget checks with retry-after evidence. Window creation rejects untrusted or
rolled-back `ClockObservation`; aliases and identifiers are normalized into one digest-bound group
key so casing/alias variants cannot bypass quota. Empty scopes, invalid limits and over-limit
requests fail closed.

The contracts are pure and in-memory. They do not reserve a durable slot, mutate usage, dispatch a
provider or claim financial authorization; BQ-08 owns durable reservation/fence/CAS.

## Evidence and limits

- `kiana-domain/tests/bq07_quota_window.rs` covers deterministic UTC windows, retry-after, clock
  rollback, alias canonicalization, empty group and over-limit behavior.
- `kiana-core/tests/bq07_quota_window_guard.rs` protects trusted-clock/no-I/O/no-provider boundary.
  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable reservation, fair
queue, lease fence, cross-process clock store or provider capacity backend is claimed.
