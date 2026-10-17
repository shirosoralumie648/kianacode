# EQ-34 performance and cost evaluator baseline

## Scope

EQ-34 adds `PerformanceCostEvaluator` to `kiana-quality`. It validates explicit duration, total
token and tool-call thresholds; checks token accounting, usage completeness and cache-key binding;
and keeps estimated, measured and unknown cost buckets distinct. Cost/latency overages and missing
or unknown measurements become stable findings rather than a pass.

The evaluator consumes bounded caller-supplied measurements only. It does not read operational
metrics, mutate a budget ledger, query a rate card, contact a provider, infer billing truth or
change scheduling/authorization.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
turns the first version into a full, reject-first performance/cost contract:

- Envelope: structural drift and an oversized bucket set fail closed in
  `PerformanceCostInput::validate` (`performance_cost_input_schema_invalid`,
  `performance_cost_cost_bucket_limit_exceeded`); per-section schema drift stays an explicit
  finding (`metrics.metrics_schema_invalid`, `metrics.threshold_schema_invalid`,
  `metrics.bucket_schema_invalid`).
- Thresholds: duration/token/tool-call bounds must be explicit and positive, and a required
  measured cost needs a positive cost limit (`metrics.threshold_missing`, the card's acceptance
  scenario `budget_and_latency_thresholds_are_explicit`); enforcing a limit without a positive
  bound is refused (`metrics.cost_limit_invalid`).
- Duration: an inferred duration is not latency evidence (`metrics.duration_unmeasured`) and an
  overage is blocking (`metrics.duration_exceeded`).
- Tokens: usage must be complete (`metrics.usage_incomplete`), `input + output` must equal the
  total without `u64` overflow (`metrics.token_total_invalid`), and the total must stay within the
  explicit threshold (`metrics.tokens_exceeded`).
- Tool calls: the observed count must stay within the explicit threshold
  (`metrics.tool_calls_exceeded`).
- Cache: lookups are hard-bounded (`metrics.cache_bounds_exceeded`), `hits + misses` must equal the
  observed lookups (`metrics.cache_count_mismatch`), cache use requires a strict `sha256:` key
  digest (`metrics.cache_key_missing`), and a key digest without cache use is unexpected
  (`metrics.cache_key_unexpected`).
- Provenance: the measurement counters are bound by a strict `sha256:` measurement digest
  (`metrics.measurement_provenance_invalid`).
- Cost buckets: one bucket per kind (`metrics.bucket_duplicate`); three-letter uppercase currency
  codes only, `UNK` exactly for unknown (`metrics.bucket_currency_invalid`); an unknown amount can
  never carry a number (`metrics.bucket_amount_invalid`); estimates need a bounded rate card and
  the strict usage digest (`metrics.bucket_estimate_unbound`); measured amounts additionally need
  the provider receipt (`metrics.bucket_measured_unbound`); unknown amounts need a bounded reason
  and must not smuggle binding evidence (`metrics.bucket_unknown_unbound`); known costs must not
  carry an unknown reason (`metrics.bucket_reason_unexpected`).
- Cost aggregates: a required measured bucket must exist (`metrics.cost_unmeasured`); an unknown
  bucket stays visible and is never folded into a numeric zero (`metrics.cost_unknown`); known
  buckets must not mix currencies (`metrics.bucket_currency_mismatch`); the declared limit must be
  enforced (`metrics.cost_unenforced`) and a known total above it is blocking with saturating
  addition (`metrics.cost_exceeded`).
- Buckets are ordered by a stable identity key before evaluation, so the finding set does not
  depend on the caller's array order while duplicate evidence stays visible. Findings carry
  per-section evidence references (`metrics:input`, `metrics:thresholds`, `metrics:duration`,
  `metrics:tokens`, `metrics:tools`, `metrics:cache`, `metrics:usage`, `metrics:cost`,
  `metrics:bucket:<index>`) and every caller-supplied value goes through `Finding::redacted`.

## Evidence and limits

- `kiana-quality/tests/eq34_metrics.rs` (26 rejection-first fixtures) covers the card's acceptance
  scenario `budget_and_latency_thresholds_are_explicit`, bounded success paths (measured and
  estimated-only), unmeasured duration, duration/token/tool overages, token accounting and
  overflow, incomplete usage, cache key/count/bounds contracts, measurement digest, measured and
  estimated bucket bindings, unknown-cost arithmetic, duplicate buckets, currency mixing, cost
  limit enforcement/overage/saturation, measured-cost requirement, schema and envelope drift,
  bounded references, oversized bucket sets, bucket order independence, the redaction boundary
  over caller values, typed round-trip, and deterministic evidence-bound registry output.
- `kiana-quality/tests/eq34_metrics_guard.rs` protects the pure bounded boundary (no
  filesystem/network/process/provider/store/broker/ledger dependency), the required markers and
  stable codes, the fixture matrix, the manual lane's exact steps and the automatic
  `kiana-quality` shard (`targets: null`).
- `.github/workflows/eq34-performance-cost.yml` runs `cargo fetch --locked`, the format gate, both
  fixture targets and two `--no-run` compile steps in GitHub Actions; local tests are intentionally
  not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim production
latency/capacity, provider tokenizer exactness, measured invoice truth, durable metric artifacts,
statistical confidence, promotion authority, live or physical evidence.

## Known limits

- Findings are diagnostics over caller-supplied values only; they never read operational metrics,
  reserve or settle a budget/ledger, query a rate card, contact a provider or change scheduling and
  authorization. Mapping them into blocking rules belongs to EQ-42.
- Durations, token counters, tool-call counts, cache hits/misses and cost amounts are
  caller-asserted evidence. The evaluator can refuse an incoherent or unbound shape, but it cannot
  prove that a real run took that long, that a tokenizer produced those tokens, or that a bucket
  matches an invoice.
- The measurement and usage digests are bounded and format-checked but never recomputed against a
  metric store, trace, receipt or rate-card revision.
- Currency handling is a single-code comparison; no FX conversion, rounding or invoice
  reconciliation is attempted.
- References are restricted to a bounded ASCII identity alphabet, so unknown shapes (including a
  bare `sk-` prefix, which the shared redactor does not rewrite) are rejected as invalid rather
  than normalized away. `Finding::redacted` covers known secret shapes; unmarked high-entropy
  values may still pass.
- Statistical aggregation, minimum sample sizes and confidence intervals belong to EQ-36; flake
  classification to EQ-37; durable EvalStore/promotion authority to later steps.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
