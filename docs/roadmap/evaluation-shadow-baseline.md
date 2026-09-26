# EQ-44 shadow admission baseline

## Scope

EQ-44 adds a bounded shadow admission and rollback evidence contract. Admission binds candidate,
baseline, route and grant digests plus sample limit and expiry. A detected regression requires a
rollback route targeting the baseline and preserves the original grant digest; sample overflow,
TTL expiry, missing rollback and grant drift are blocking findings.

The evaluator validates evidence only. It does not start a shadow target, switch a provider route,
change a grant, execute rollback, or write a durable gate/candidate state.

## Evidence and limits

- `kiana-quality/tests/eq44_shadow.rs` covers valid regression rollback, missing/wrong route or
  grant, sample/TTL bounds and strict unknown fields.
- `kiana-quality/tests/eq44_shadow_guard.rs` protects sample/TTL/grant fences and no-execution
  boundary.
- `.github/workflows/eq44-shadow.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=partial`, `proof_level=source`: no real shadow traffic, route switch,
durable rollback record, ControlPlane effect, live or physical evidence is claimed.
