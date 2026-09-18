# BQ-05 RateCardStore Baseline

## Scope

BQ-05 adds the `RateCardStore` port and bounded `InMemoryRateCardStore` fixture adapter. Cards are
resolved by provider/model and a caller-supplied timestamp, pinned by `RateCardId`/`card_version`,
and rejected when effective windows overlap. Unknown model, expired card, duplicate version and
invalid lookup are stable failures; no implicit “current price” fallback exists.

`RateCard` now exposes explicit dimensions for cache, reasoning, audio input/output, requests,
tools and effects. `CostEstimate` carries the pinned `rate_card_version`; it remains an estimate,
not a provider invoice or financial authorization.

## Evidence and limits

- `kiana-ports/tests/bq05_rate_card_store.rs` covers pinned resolution, dimension mapping,
  audio/effect pricing, overlap, unknown-model and expiry denial.
- `kiana-core/tests/bq05_rate_card_guard.rs` protects the port-only/no-provider/no-financial-
  authority boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable rate-card projector,
provider discovery, invoice import, currency conversion or reservation admission is claimed.
