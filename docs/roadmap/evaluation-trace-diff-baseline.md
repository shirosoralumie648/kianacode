# EQ-21 Trace Diff Baseline

## Scope

EQ-21 adds a deterministic pure `TraceDiff` over EQ-19 normalized traces. It checks normalization
version and array-policy comparability first, then walks events in source order and reports only
the first divergence: cursor, event kind, missing/extra event, nested JSON field, terminal index
or trace metadata. Each finding carries event index/cursor, a stable field path, a classification
and bounded redacted expected/actual summaries. Provenance request IDs are not compared because
they are outside normalized event values.

The diff does not decide quality, alter traces, call a provider, persist a report or authorize
promotion. Later assertion/evaluator steps consume its structured result.

## Evidence and limits

- `kiana-quality/tests/eq21_trace_diff.rs` covers nested first divergence, cursor/kind/length/
  version classification, identical provenance handling, terminal metadata and summary redaction.
- `kiana-quality/tests/eq21_trace_diff_guard.rs` protects the pure bounded-summary boundary.
- `.github/workflows/eq21-trace-diff.yml` runs the fixtures, source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: first-divergence source and CI
fixtures are present, while assertion DSL, evaluator findings, capture and durable quality
evidence remain later EQ steps.
