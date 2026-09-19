# EQ-24 Reference/Candidate Scenario Baseline

## Scope

EQ-24 adds a pure reference/candidate scenario comparison contract. Both artifacts bind the same
scenario ID and a scrubbed environment snapshot; absolute workspace/home paths, raw secret
sentinels and unsafe environment values are rejected. The runner computes normalized trace digests,
uses `TraceDiff`, applies an explicit `AllowAll` or field-prefix scope predicate, and requires a
cleanup receipt. Environment mismatch is `not_comparable`, incomplete cleanup is
`cleanup_required`, and an out-of-scope first divergence is visible rather than silently accepted.

This is a comparison/reporting contract, not a process runner. It does not create a workspace,
read a fixture, start a provider, execute a candidate, clean the host or persist a report.

## Evidence and limits

- `kiana-quality/tests/eq24_scenario.rs` covers scrubbed-environment binding, in-scope diff,
  environment mismatch, cleanup-required, out-of-scope and unsafe-value rejection.
- `kiana-quality/tests/eq24_scenario_guard.rs` protects the no-filesystem/no-runner boundary.
- `.github/workflows/eq24-scenario.yml` runs the fixtures, source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: scenario comparison source and CI
fixtures are present, while actual reference/candidate execution, environment lifecycle, scope
authorization and durable evidence remain later work.
