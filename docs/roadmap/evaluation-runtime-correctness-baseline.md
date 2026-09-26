# EQ-28 runtime correctness evaluator baseline

## Scope

EQ-28 adds `RuntimeCorrectnessEvaluator` to `kiana-quality`. It consumes the canonical,
redacted trace produced by EQ-17/EQ-18 and emits the existing EQ-27 bounded `Finding` shape.
The evaluator checks source-cursor and per-request sequence order, correlation and invocation
identity aliases, one terminal per stream/invocation, approval request/decision/consumption order,
retry metadata/order, cancellation fencing and terminal completion, and the rule that an
`Unknown` result must be reconciled before a new attempt.

The input is caller-supplied evidence only. The evaluator does not read EventLog state, start a
runner, contact a provider, dispatch a capability, authorize an approval, cancel a run, or
schedule a retry. Findings are diagnostics for later quality aggregation and are not authority.

## Evidence and limits

- `kiana-quality/tests/eq28_runtime.rs` covers a valid approval/invocation/run lifecycle,
  unmatched and duplicate terminal events, Unknown/cancel fencing, and malformed input.
- `kiana-quality/tests/eq28_runtime_guard.rs` protects the pure bounded boundary and required
  runtime finding markers.
- `.github/workflows/eq28-runtime-correctness.yml` runs the fixtures, source guard, formatting and
  workspace test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim durable
EvalStore facts, real target execution, provider/model quality results, statistical aggregation,
promotion authority, cross-process recovery, live or physical evidence. Those remain EQ-29+ and
ER/PD/SC work.
