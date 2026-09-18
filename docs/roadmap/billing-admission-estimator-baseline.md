# BQ-09 Admission Estimator Baseline

## Scope

BQ-09 adds a pure admission upper-bound estimator for the final wire-request digest, retry
allowance, output cap, tool/effect calls and storage bytes. Token basis is explicit: exact tokenizer
or conservative bytes upper bound is marked accordingly, while unknown tokenizer cannot be presented
as exact. A hard cost limit rejects unknown price/usage instead of inventing a zero cost; estimates
carry pinned rate-card ID/version when available.

The estimator is a pre-effect calculation only. It does not reserve quota, dispatch a provider,
authorize a Broker, or persist a financial ledger.

## Evidence and limits

- `kiana-domain/tests/bq09_admission_estimator.rs` covers retry/output/tool/effect/storage bounds,
  exact-vs-upper basis, missing price/hard-cost denial and malformed limits.
- `kiana-core/tests/bq09_admission_guard.rs` protects the pure/no-provider/no-financial-authority
  boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable quota reservation,
provider request admission, actual tokenizer or invoice evidence is claimed.
