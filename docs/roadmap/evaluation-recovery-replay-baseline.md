# EQ-31 recovery and replay evaluator baseline

## Scope

EQ-31 adds `RecoveryReplayEvaluator` to `kiana-quality`. It checks crash/restart evidence,
monotonic replay cursors, explicit resume, old/new fence separation, logic-version compatibility,
replay digest divergence, and `result_unknown` reconciliation rules. Replay divergence and unknown
external outcome are emitted as separate stable findings; an unknown result requires a reconciliation
reference and never permits retry before reconciliation.

The evaluator consumes evidence only. It does not restart a process, read or rebuild an EventLog,
revoke or issue a fence, query an external effect, reconcile an operation, or authorize a retry.

## Evidence and limits

- `kiana-quality/tests/eq31_recovery.rs` covers equivalent restart replay, distinct divergence and
  Unknown findings, crash/restart fence/resume denial, and strict unknown-field rejection.
- `kiana-quality/tests/eq31_recovery_guard.rs` protects the pure recovery boundary and separate
  divergence/Unknown markers.
- `.github/workflows/eq31-recovery-replay.yml` runs fixtures, source guard, formatting and
  workspace test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim real crash or
power-loss recovery, durable checkpoint/replay, external reconciliation, provider receipts,
cross-process fence proof, promotion authority, live or physical evidence.
