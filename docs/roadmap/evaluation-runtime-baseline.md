# EQ-09 Isolated Evaluation Runtime Baseline

## Scope

EQ-09 adds `kiana-daemon::eval_runtime::EvalRuntimeSandbox`, a bounded adapter for deterministic
evaluation fixtures.  Each sandbox creates a fresh, exact temporary root with separate
`workspace/` and `kiana-home/` directories, exposes an explicit `KIANA_HOME`/`HOME` environment
map, records a fixed `ClockObservation`, and derives deterministic non-security fixture bytes from
a supplied seed.  A serialized process-environment callback is available for CI fixtures and
restores all previous values even when the callback returns an error.

The adapter never discovers the operator home, starts KianaHarness, creates a scheduler/Broker,
calls a provider, or creates an evaluator loop.  Evaluation targets must still enter the existing
DaemonHost/ControlPlane path in later EQ-12 work.

## Evidence and limits

- `kiana-daemon/tests/eq09_eval_runtime.rs` covers root containment, fixed clock/seed, deterministic
  bytes, environment isolation and restoration.
- `kiana-core/tests/eq09_eval_runtime_guard.rs` protects the no-runner/no-network/no-second-loop
  boundary.  GitHub Actions runs the fixtures; local tests are intentionally not executed.
- Temporary cleanup is best-effort after the sandbox drops; a cleanup failure is not converted into
  an eval pass and should be surfaced by the caller.

This slice is `feature_status=implemented`, `proof_level=source`: it does not yet instantiate a
DaemonHost target, fake provider, deny broker, fixture store, event capture, restart/reconcile or
external/live effect. Those remain EQ-10+; no live/physical proof is claimed.

