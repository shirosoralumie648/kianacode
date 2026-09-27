# SW-09 progress ledger and stall escalation baseline

## Scope

SW-09 adds a bounded observation contract for heartbeat sequence, checkpoint sequence, turns,
tokens, effects, wall time, stalls and the corresponding per-run budget. The ledger rejects
identity drift, sequence/counter rollback and budget overrun. Reaching a stall or hard budget bound
while still marked Running is rejected; the caller must surface Stalled/Escalated/Exhausted.

This contract records progress only. It does not start a runner, schedule heartbeat timers, dispatch
effects, persist checkpoints or escalate to an external operator.

## Evidence and limits

- `kiana-domain/tests/swarm_progress.rs` covers monotonic progress, budget/stall gates and strict
  digest/field rejection.
- `kiana-core/tests/swarm_progress_guard.rs` protects the bounded observation/no-effect boundary.
- `.github/workflows/sw09-progress.yml` runs fixtures, source guard, formatting and affected target
  compilation in GitHub Actions; local Cargo tests/build/check/clippy/smoke commands were not run.

This slice is `feature_status=partial`, `proof_level=source`: persisted progress, real heartbeat,
checkpoint, stall timer/escalation, hierarchical runtime budgets and recovery remain unproven.
