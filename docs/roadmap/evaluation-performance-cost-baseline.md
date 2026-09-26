# EQ-34 performance and cost evaluator baseline

## Scope

EQ-34 adds `PerformanceCostEvaluator` to `kiana-quality`. It validates explicit duration, total
token and tool-call thresholds; checks token accounting, usage completeness and cache-key binding;
and keeps estimated, measured and unknown cost buckets distinct. Cost/latency overages and missing
or unknown measurements become stable findings rather than a pass.

The evaluator consumes bounded caller-supplied measurements only. It does not read operational
metrics, mutate a budget ledger, query a rate card, contact a provider, infer billing truth or
change scheduling/authorization.

## Evidence and limits

- `kiana-quality/tests/eq34_metrics.rs` covers measured metrics with explicit thresholds, duration/
  token/tool/cache/usage/cost violations, unknown cost, missing thresholds and strict fields.
- `kiana-quality/tests/eq34_metrics_guard.rs` protects the pure evaluator and estimated/measured/
  unknown bucket boundary.
- `.github/workflows/eq34-performance-cost.yml` runs fixtures, source guard, formatting and
  workspace test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim production
latency/capacity, provider tokenizer exactness, measured invoice truth, durable metric artifacts,
statistical confidence, promotion authority, live or physical evidence.
