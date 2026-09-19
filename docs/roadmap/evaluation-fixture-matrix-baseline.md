# EQ-26 Provider-Independent Fixture Matrix Baseline

## Scope

EQ-26 adds a bounded core negative fixture matrix for Runtime, Approval, Hook, Memory, Workflow
and Swarm. Every fixture is a normalized trace with a stable family/id, blocked expected status,
explicit forbidden-effect codes, `provider_calls=0`, `side_effects=false` and a digest. The matrix
validator requires exactly one entry for each family and rejects missing/duplicate coverage.

The fixtures are provider-independent value contracts. They do not execute a model/provider,
open a workspace, contact a Broker, discover a fixture file or establish durable/live evidence.

## Evidence and limits

- `kiana-quality/tests/eq26_fixtures.rs` covers complete six-family matrix, duplicate/missing
  family rejection, digest binding and forbidden-effect metadata.
- `kiana-quality/tests/eq26_fixtures_guard.rs` protects provider-independent/no-effect boundaries.
- `.github/workflows/eq26-fixtures.yml` runs the fixtures, source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: fixture matrix source and CI
fixtures are present, while actual target execution, EventLog capture, evaluator aggregation and
durable quality evidence remain later work.
