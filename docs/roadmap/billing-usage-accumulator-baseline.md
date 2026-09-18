# BQ-03 Usage Accumulator Baseline

## Scope

BQ-03 adds a pure attempt-local `UsageAccumulator`. It deduplicates equal sequence/digest pairs,
rejects same-sequence conflicts and sequence regression, applies deltas with checked arithmetic,
requires a known snapshot before a delta, and enforces component-wise cumulative containment for
snapshots/final observations. A final observation closes the accumulator against later writes.

Unknown/absent fields are never synthesized into zero: a delta cannot add to an unknown optional
counter, and overflow or dropped known fields fail closed. The reducer does not write EventLog,
reserve quota, call a provider or settle a bill.

## Evidence and limits

- `kiana-domain/tests/bq03_usage_accumulator.rs` covers snapshot/delta/final success, duplicate
  replay, sequence conflict/regression, containment, unknown base, overflow and post-final denial.
- `kiana-core/tests/bq03_usage_accumulator_guard.rs` protects the pure/no-I/O/no-provider boundary.
  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: durable attempt records, provider
normalization adapters, quota reservation, settlement and cross-process recovery remain BQ-08+.
