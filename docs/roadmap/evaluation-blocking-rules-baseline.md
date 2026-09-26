# EQ-42 quality blocking rules baseline

## Scope

EQ-42 adds a fixed blocking-rule matrix for safety, evidence, replay, forbidden effects, fixture
integrity and infrastructure. Every rule must be present; triggered rules require finding refs and
take precedence over the weighted score. An insufficient score also cannot be declared Pass.

This evaluator is read-only: it does not mutate QualityGate configuration, execute a target,
authorize a capability, or promote/rollback a candidate.

## Evidence and limits

- `kiana-quality/tests/eq42_rules.rs` covers a complete passing matrix, high score with safety
  blocker, below-threshold false Pass, missing rule and strict fields.
- `kiana-quality/tests/eq42_rules_guard.rs` protects precedence-first behavior and pure boundary.
- `.github/workflows/eq42-blocking-rules.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim an immutable
persisted GateDecision, ControlPlane promotion authority, durable EvalStore, live or physical proof.
