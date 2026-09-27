# SW-06 queue, fairness and claim fencing baseline

## Scope

SW-06 adds a bounded queue ledger over dispatch intent references. Entries have stable sequence,
session fairness, capacity, delay/backoff, worker epoch and explicit claim/complete/fenced states.
Claiming is a queue reservation only; effect-time authorization remains a separate ControlPlane /
Broker boundary. Expired epochs, capacity oversell, delayed entries and unbounded retry are
rejected.

The source contract is compatible with the preceding SW-05 admission receipt but does not claim a
durable EventLog-backed queue or cross-process lease CAS.

## Evidence and limits

- `kiana-domain/tests/swarm_queue.rs` covers deterministic order, capacity, delay, stale fence,
  bounded retry/max-attempt fencing, digest drift and unknown fields.
- `kiana-core/tests/swarm_queue_guard.rs` protects the ready/claim boundary from provider, runner,
  filesystem or effect execution.
- `.github/workflows/sw06-queue-claim.yml` runs the fixtures, source guard, formatting and target
  compilation in GitHub Actions; local Cargo tests/build/check/clippy/smoke commands were not run.

This slice is `feature_status=partial`, `proof_level=source`: durable DispatchIntent/QueueEntry
replay, multi-process fairness, lease persistence, restart recovery and live/physical effects
remain unproven.
