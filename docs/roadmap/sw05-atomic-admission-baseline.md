# SW-05 atomic swarm admission baseline

## Scope

SW-05 adds a deterministic admission contract that checks budget, path locks, data locks, claim,
grant, supervision and dispatch intent before one mutation block. The bounded ledger returns the
original receipt for an identical idempotency payload, rejects payload conflicts and active work
fingerprints, and leaves every resource unchanged when a preflight fault is supplied.

The Core facade is read-only around this domain contract. It does not start a child, invoke a
Broker, append an EventLog, or claim durable cross-process CAS.

## Evidence and limits

- `kiana-domain/tests/swarm_admission.rs` covers all-resource reservation/replay, idempotency and
  active-fingerprint conflict, atomic no-partial-failure, path/data overlap and strict fields.
- `kiana-core/tests/swarm_admission_guard.rs` protects the single ControlPlane boundary and no
  effect path.
- `.github/workflows/sw05-atomic-admission.yml` runs the fixtures, source guard, formatting and
  affected test-target compilation in GitHub Actions; local Cargo tests/build/check/clippy/smoke
  commands were not run.

This slice is `feature_status=partial`, `proof_level=source`: the in-memory ledger is a source
contract only. Durable EventLog CAS, multiple processes, real worker claims, child execution,
restart/recovery and live/physical effects remain SW-06+ and ER/PD work.
