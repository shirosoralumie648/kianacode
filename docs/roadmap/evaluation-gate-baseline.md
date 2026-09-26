# EQ-41 QualityGate configuration/decision baseline

## Scope

EQ-41 adds separate immutable `QualityGateConfig` and `QualityGateDecision` value contracts. A
decision binds the exact gate config digest, candidate digest, verdict and blocking findings;
configuration updates require a higher version and a distinct config digest, while old decisions
remain bound to the original config. A Pass with blocking findings is rejected.

This is a pure contract/evaluator. It does not persist a gate, mutate an old decision, authorize a
route, promote/rollback a candidate or execute a target.

## Evidence and limits

- `kiana-quality/tests/eq41_gate.rs` covers valid config/decision separation, config update
  immutability, blocking findings, digest tamper and strict unknown fields.
- `kiana-quality/tests/eq41_gate_guard.rs` protects the config/decision/no-authority boundary.
- `.github/workflows/eq41-gate.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable gate store, immutable
decision history, ControlPlane authorization, promotion/rollback, live or physical evidence is
claimed.
