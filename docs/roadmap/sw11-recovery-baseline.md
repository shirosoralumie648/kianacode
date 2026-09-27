# SW-11 replay and re-admission baseline

## Scope

SW-11 adds a strict replay fact for pending writes. It binds source and committed cursors, pending
write digest, source event IDs, authority epoch and explicit recovery state. A ResultUnknown fact
preserves uncertain effects; Recovered requires a known effect and explicit re-admission authority.

The contract is evidence-only. It does not read or append EventLog frames, restart workers, retry an
effect, or turn an unknown result into success.

## Evidence and limits

- `kiana-domain/tests/swarm_recovery.rs` covers pending/unknown visibility, explicit re-admission,
  cursor bounds, digest and unknown-field rejection.
- `kiana-core/tests/swarm_recovery_guard.rs` protects the no-auto-success/no-effect boundary.
- `.github/workflows/sw11-recovery.yml` runs fixtures, source guard, formatting and affected target
  compilation in GitHub Actions; local Cargo tests/build/check/clippy/smoke commands were not run.

This slice is `feature_status=partial`, `proof_level=source`: durable replay, new-process recovery,
EventLog CAS, external effect reconciliation and live/physical outcomes remain unproven.
