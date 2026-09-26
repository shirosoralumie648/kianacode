# EQ-36 dimension aggregation baseline

## Scope

EQ-36 adds a bounded `AggregationEvaluator` for per-dimension sample count, absolute and relative
thresholds, and confidence intervals. Dimension evidence has an explicit minimum sample count;
insufficient samples are only valid as `Blocked` or `NeedsReview`, never `Pass`. Confidence bounds
are explicit when required, and a passing verdict that conflicts with blocking evidence emits a
false-pass finding.

The evaluator does not run experiments, calculate statistical estimates from raw samples, update a
QualityGate, or promote a candidate. It validates caller-supplied aggregate evidence only.

## Evidence and limits

- `kiana-quality/tests/eq36_aggregation.rs` covers sufficient samples, insufficient sample verdict,
  absolute/relative limits, confidence interval, missing baseline and strict fields.
- `kiana-quality/tests/eq36_aggregation_guard.rs` protects the pure evaluator and explicit
  sample/threshold/confidence boundaries.
- `.github/workflows/eq36-aggregation.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim statistical
validity of upstream samples, production quality, durable experiment state, gate immutability,
promotion authority, live or physical evidence.
