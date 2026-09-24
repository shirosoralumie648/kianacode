# BQ-18 · Allow-listed fallback and route/authority/data/price re-admission baseline

This source slice keeps provider fallback behind the existing
`DaemonHost → ControlPlane → ModelBudgetPort/Broker → ProviderGateway` spine.  A fallback is a
new attempt with a new route, budget reservation and opaque permit; it is not a provider-side
retry loop and it cannot inherit the old attempt's unknown or receipt.

## Source contract

- `FallbackRouteAllowlist` pins exact route digests together with provider and model identity.
  Profile aliases and a model name alone cannot select a route.
- `FallbackAdmissionRequest` carries the server-owned run/attempt, authority epoch, data-boundary
  digest, effective budget, original reservation and original permit.  `FallbackAdmissionCandidate`
  must provide the same current authority/data/budget facts, a fresh budget reservation, credential
  revision, an allow-listed route and an effective `RateCard` for that provider/model.
- `admit_fallback_attempt` rejects capability downgrades, authority/data drift, missing credential,
  missing budget or price, stale authority, cross-boundary candidates and reuse of the primary
  permit/reservation.  It returns a `FallbackAttemptAdmission` with fresh attempt, route,
  authority/data/credential, rate-card and permit digests.
- `FallbackAttemptReceipt` binds one normalized usage observation and one cost breakdown to the
  admitted attempt.  Estimated, measured and unknown outcomes remain separate; measured requires
  an opaque provider receipt, and an unknown outcome has no success conversion.  The process-local
  `FallbackReceiptLedger` only provides idempotent replay/conflict detection.
- Core exposes only a ControlPlane re-admission wrapper.  Provider exposes validation of the
  ControlPlane result and does not choose fallback, mint permits, call EventLog or start another
  execution loop.

## CI-only fixtures

GitHub Actions `bq18-fallback-admission.yml` runs domain deny-first/independent-receipt fixtures,
provider boundary source fixtures, and Core source guards, followed by workspace test-target
compilation.  Local Cargo tests, builds, checks, clippy and smoke commands are intentionally not
run.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source`.  The allowlist and receipt
ledger are in-process value contracts; this does not claim durable cross-process authority/CAS,
provider credential validity, EventLog append/flush, provider invoice reconciliation, external
network behavior or live/physical side effects.  Existing `provider_capacity::admit_fallback`
remains a compatibility contract for P4-J7-25; BQ-18 adds the stricter authority/data/price and
per-attempt receipt boundary without adding a second provider loop.  CI results are intentionally
not awaited.
