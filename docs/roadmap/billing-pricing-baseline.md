# BQ-04 Money / RateCard Baseline

## Scope

BQ-04 adds integer-micros `Money`, checked addition/multiplication, versioned/effective `RateCard`
contracts and a bounded `CostEstimate`. Currency is explicit, rate-card identity/version/source
and effective windows are digest-bound, and arithmetic overflow/currency mismatch fail closed.

Missing usage or a missing unit price produces an explicit unknown reason (`partial` or
`rate_card_missing`); it never becomes zero. Estimates are not provider invoices or financial
authorization. Later BQ-05 owns a rate-card store and mapping; BQ-13 owns measured/estimated
receipt breakdown.

## Evidence and limits

- `kiana-domain/tests/bq04_pricing.rs` covers integer micros, checked arithmetic, currency/window/
  digest validation and explicit unknown estimates.
- `kiana-core/tests/bq04_pricing_guard.rs` protects the pure integer/no-float/no-provider boundary.
  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no rate-card store, provider
invoice, currency conversion, reservation admission or durable cost ledger is claimed.
