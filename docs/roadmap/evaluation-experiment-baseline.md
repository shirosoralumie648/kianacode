# EQ-38 EvalExperiment replay baseline

## Scope

EQ-38 adds a pure replayable `EvalExperiment` contract to `kiana-quality`. Admission, start,
case-completed and terminal events reduce deterministically into an experiment status and a
case-result index. Sequence gaps, missing admission/terminal, illegal transitions, duplicate case
result digest drift and events after terminal fail closed; the resulting experiment carries a
replay digest.

This is a value/reducer contract only. It does not persist to EvalStore, run cases, invoke a
provider/judge, allocate budget, or submit a quality-gate decision.

## Evidence and limits

- `kiana-quality/tests/eq38_experiment.rs` covers replayable completed state/index, case-result
  conflict, post-terminal events, missing terminal and unknown fields.
- `kiana-quality/tests/eq38_experiment_guard.rs` protects the pure replay/index boundary.
- `.github/workflows/eq38-experiment.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable experiment/result
store, real target execution, provider/judge evidence, gate decision, promotion, live or physical
proof is claimed.
