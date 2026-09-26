# EQ-35 semantic judge contract baseline

## Scope

EQ-35 adds a strict quality-side semantic-judge evidence contract around the existing
`kiana-ports::Judge` boundary. It fixes provider/model/prompt/temperature/configuration identity,
binds input/reference/output digests and result digest, and distinguishes Available, Unavailable
and NotConfigured. A required unavailable judge, or a forged Pass while unavailable, is blocking;
judge output remains a quality diagnostic and never changes authorization or promotion.

No real model, account, network or judge adapter is invoked by this slice. The contract is ready
for a later explicitly configured adapter while preserving the no-judge-is-pass boundary.

## Evidence and limits

- `kiana-quality/tests/eq35_judge.rs` covers fixed available evidence, unavailable-not-pass,
  configuration/result drift, bounds and strict unknown-field rejection.
- `kiana-quality/tests/eq35_judge_guard.rs` protects the optional/no-authority boundary.
- `.github/workflows/eq35-semantic-judge.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no semantic quality claim,
external judge result, durable EvalStore record, promotion, live or physical evidence is made.
