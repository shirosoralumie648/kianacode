# EQ-36 dimension aggregation baseline

## Scope

EQ-36 adds a bounded `AggregationEvaluator` for per-dimension sample count, absolute and relative
thresholds, and confidence intervals. Dimension evidence has an explicit minimum sample count;
insufficient samples are only valid as `Blocked` or `NeedsReview`, never `Pass`. Confidence bounds
are explicit when required, and a passing verdict that conflicts with blocking evidence emits a
false-pass finding.

The evaluator does not run experiments, calculate statistical estimates from raw samples, update a
QualityGate, or promote a candidate. It validates caller-supplied aggregate evidence only.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
turns the first version into a full, reject-first aggregation contract:

- Envelope: a drifted schema, an empty dimension set and an oversized dimension list fail closed
  in `AggregationInput::validate` (`aggregation_input_schema_invalid`,
  `aggregation_input_dimensions_missing`, `aggregation_dimension_limit_exceeded`); per-dimension
  schema drift stays an explicit finding (`aggregation.dimension_schema_invalid`).
- Dimension identity: the name must be a bounded, single-line, secret-free token and unique across
  the envelope (`aggregation.dimension_invalid` for an empty, oversized, malformed or duplicated
  identity). Duplicates are refused instead of being silently merged, and an unbounded or
  secret-shaped identity is replaced by `invalid` before it can reach a finding.
- Minimum sample: the minimum must be positive (`aggregation.minimum_sample_invalid`), an
  insufficient sample is blocking (`aggregation.insufficient_sample`) and it may only carry a
  `blocked` or `needs_review` verdict (`aggregation.sample_verdict_invalid`). The card acceptance
  `insufficient_sample_is_blocked_or_needs_review` covers this boundary.
- Thresholds: every dimension must declare at least one explicit absolute or relative limit
  (`aggregation.threshold_missing`); an absolute overage is blocking
  (`aggregation.absolute_threshold_exceeded`); a relative limit needs a nonzero baseline
  (`aggregation.relative_threshold_invalid`) and an exceeded relative delta is blocking
  (`aggregation.relative_threshold_exceeded`, per-mille integer arithmetic).
- Confidence interval: a required interval must exist (`aggregation.confidence_missing`); a
  provided interval must be a complete ordered pair that contains the observed value
  (`aggregation.confidence_invalid` for inverted, half or non-containing bounds, whether or not
  confidence is required).
- Verdict: a `pass` verdict that conflicts with any blocking evidence emits
  `aggregation.false_pass`; `fail`/`needs_review`/`blocked` claims stay visible and are never
  collapsed into success. Dimensions are sorted by a stable full-record key before evaluation, so
  findings and evidence indices do not depend on the caller's array order.
- Findings cite the evidence section they came from (`aggregation:dimension:<i>`,
  `aggregation:verdict`) and every caller-supplied value goes through `Finding::redacted`.

## Evidence and limits

- `kiana-quality/tests/eq36_aggregation.rs` expands the original four fixtures to 19
  rejection-first fixtures: the card acceptance `insufficient_sample_is_blocked_or_needs_review`,
  the bounded success path, schema drift, bounded/unique identities, zero minimum sample, missing
  threshold policy, absolute and relative overages (including below-baseline drift and the
  zero-baseline pair), required/optional/missing/inverted/half/non-containing confidence
  intervals, the false-pass matrix under every blocking family, envelope schema/limit/unknown
  field rejection, order independence, the redaction boundary, typed round-trip and registry
  digest wiring.
- `kiana-quality/tests/eq36_aggregation_guard.rs` protects the pure evaluator (no
  filesystem/network/process/store/broker dependency), the required markers and stable codes, the
  fixture matrix, the manual lane's exact steps and the automatic `kiana-quality` shard
  (`targets: null`).
- `.github/workflows/eq36-aggregation.yml` runs `cargo fetch --locked`, the format gate, both
  fixture targets and two `--no-run` compile steps in GitHub Actions; local tests are intentionally
  not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim statistical
validity of upstream samples, production quality, durable experiment state, gate immutability,
promotion authority, live or physical evidence.

## Known limits

- The evaluator validates caller-supplied aggregate evidence; it never runs an experiment, samples
  a workload or recomputes an observation from raw data, so it cannot prove that a sample count,
  baseline or confidence interval reflects a real measurement.
- Per-mille integer arithmetic avoids floating point, but rounding is per-mille division and is not
  a statistical confidence computation; a caller can still supply an interval that is numerically
  ordered yet statistically meaningless.
- Bounded identities are a shape check (ASCII allow-list plus `redact_text` comparison); a
  high-risk opaque string without a known secret marker (for example a bare `sk-` prefix) can pass
  the allow-list, consistent with the shared redaction boundary. Unbounded or secret-shaped
  identities are replaced by `invalid` before reaching a finding.
- `aggregation.false_pass` and the other findings are diagnostics; mapping them into blocking rules
  and weighted scores belongs to EQ-42, and flake classification to EQ-37.
- No durable EvalStore record, gate mutation, candidate advancement, promotion path, live or
  physical evidence is added here.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
