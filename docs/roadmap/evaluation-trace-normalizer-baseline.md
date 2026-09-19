# EQ-17 Durable Event Selection Baseline

## Scope

EQ-17 introduces the pure `kiana-quality` normalizer boundary for a bounded, source-ordered slice
of committed `RuntimeEvent` values. It rejects invalid/duplicate source cursors and event IDs,
non-monotonic request sequences or aggregate stream versions, missing or drifting correlation,
unknown required event kinds, dangling causal/parent references and more than one terminal event
per aggregate stream. Global EventLog cursor gaps are allowed because a run-scoped selection may
skip unrelated aggregates.

The result is still a structural durable-event selection. Canonical JSON, redaction, declared
volatile replacement, event/trace digests, diffing and evaluator findings belong to EQ-18+; the
quality crate does not start a runner, open a store, contact a provider or dispatch an effect.

## Evidence and limits

- `kiana-quality/tests/eq17_normalize.rs` covers source-order selection, cursor rejection,
  multiple-terminal rejection, sequence/correlation drift and causal-reference rejection.
- `kiana-quality/tests/eq17_quality_guard.rs` protects the pure/no-Tokio/no-network/no-filesystem
  boundary.
- `.github/workflows/eq17-trace-normalizer.yml` runs the fixtures, source guard and workspace
  compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the normalizer source and CI
fixtures are present, while canonical/redacted/volatile normalized traces and durable EvalStore
evidence are not claimed.
