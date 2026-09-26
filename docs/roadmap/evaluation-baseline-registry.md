# EQ-39 baseline registry baseline

## Scope

EQ-39 adds a bounded baseline registry and compatibility comparison contract. Each baseline binds
suite, case, target and evaluator digests, owner, creation/expiry and refresh provenance; registry
identity and entries are digest checked. Comparison rejects expired/tampered baselines and any
suite/case/target/evaluator drift before producing a comparable result.

The evaluator is read-only evidence validation. It does not refresh a baseline, execute a target,
persist an EvalStore record, invoke a judge, alter a candidate or submit a gate decision.

## Evidence and limits

- `kiana-quality/tests/eq39_baseline.rs` covers compatible comparison, stale/incompatible rejection,
  duplicate/tampered registry entries and strict unknown fields.
- `kiana-quality/tests/eq39_baseline_guard.rs` protects expiry/digest/provenance boundaries.
- `.github/workflows/eq39-baseline.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable baseline store,
operator ownership/authentication, refresh command, target execution, gate/promotion or live/
physical evidence is claimed.
