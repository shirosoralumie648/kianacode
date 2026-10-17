# BQ-18 · Allow-listed fallback and route/authority/data/price re-admission baseline

This source slice keeps provider fallback behind the existing
`DaemonHost → ControlPlane → ModelBudgetPort/Broker → ProviderGateway` spine.  A fallback is a
new attempt with a new route, budget reservation and opaque permit; it is not a provider-side
retry loop and it cannot inherit the old attempt's unknown or receipt.

## Source contract

- `FallbackRouteAllowlist` pins exact route digests together with provider, connection and model
  identity. Profile aliases and a model name alone cannot select a route. The connection identity
  is a required field; older serialized entries without it are rejected by strict deserialization
  and must be rebuilt from a trusted route snapshot.
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

## 2026-10-08 connection identity binding

The fallback boundaries now reject a candidate that changes only `ModelRoute.connection_id`.
`FallbackRouteAllowlistEntry` stores and validates the connection identity, includes it in
canonical ordering and duplicate detection, and compares it in `contains`. The provider-side
`validate_fallback_attempt` check performs the same explicit comparison while preserving the
existing route digest compatibility. This is a local compatibility slice rather than a global
`ModelRoute::digest()` migration, so persisted route digests outside fallback are unchanged.

```text
source_snapshot: current dirty worktree; kiana-domain/src/fallback_admission.rs; kiana-domain/tests/bq18_fallback_admission.rs; kiana-provider/src/fallback.rs; kiana-provider/tests/bq18_fallback_admission.rs; kiana-core/tests/bq18_fallback_admission_guard.rs
worktree_status: BQ-18 row 488 remains 🔄 / feature_status=implemented / proof_level=source; cross-connection fallback is denied at domain allowlist and provider validation boundaries
command_argv: RED domain `allowlist_rejects_connection_identity_drift`; GREEN domain full `bq18_fallback_admission`; provider unit `fallback::tests::validate_rejects_connection_identity_drift`; provider guard target; Core guard target; protected replay and route attestation regression targets
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB test stack, stripped/non-incremental profile; domain RED 240688 KiB / 32% CPU; domain GREEN 4079904 KiB / 83% CPU; domain final selector 83016 KiB / 23% CPU; provider behavior 4058288 KiB / 79% CPU; provider guard 4049656 KiB / 84% CPU; Core guard 1197184 KiB / 86% CPU; protected replay/route attestation 254728 KiB / 93% CPU; all sampled runs reported 0 swaps
fixture·cassette: same provider/model with changed connection; two distinct connections can both be allowlisted; missing `connection_id` in legacy serialized entry is rejected; domain BQ-18 6/6, provider behavior 1/1 plus guard 2/2, Core guard 2/2, protected replay 3/3 and route attestation 5/5
exit_code: RED 101 expected; all focused/regression checks 0; prior full BQ-18 fixture failures were cleared by correcting unrelated fixture bindings (wildcard card for model drift and matching rate-card identity/version)
status_change: selected cross-connection fallback denial moves source → local_behavior; whole BQ-18 remains implemented/source
proof-level_change: allowlist/provider connection identity source → local_behavior; no durable CAS, Gateway hot-path invocation, provider credential/invoice, live or physical promotion
limitations: `ModelRoute::digest()` remains unchanged for persisted compatibility; old fallback entries lacking `connection_id` are rejected and require trusted regeneration; provider validator is still exported without a production call site, and protected replay/route attestation are regression-only boundaries
reviewer: root integration agent with CI-08/BQ-18 fallback audit; focused connection-identity reviewer
```

## 2026-10-17 completion slice (CI-only)

The card's refusal surface — capability downgrade, cross data-boundary fallback, missing
credential/budget/price, stale authority, allowlist tamper and reuse of the primary
permit/reservation — and the success column (every fallback attempt owns its route, usage,
pinned rate card and receipt) are now executable fixtures instead of prose:

- `FallbackAdmissionCandidate` splits the price refusal into a typed `fallback_rate_card_expired`
  (window closed at `now_unix_ms`) and `fallback_rate_card_scope_mismatch` (provider/model
  selector drift), so an expired card can no longer be reported as a scope problem.
- `FallbackAttemptAdmission` records `original_route_digest` and `allowlist_digest`, which makes the
  "white-listed fallback" claim auditable per attempt: the admission must link a different route
  than the primary one, and it pins the exact allowlist revision that admitted it. Both digests
  participate in `admission_digest`.
- `FallbackAttemptReceipt` copies the admitted route's `provider_id`/`connection_id`/`model_id` and
  requires the normalized usage to match all three, the receipt's cost breakdown to carry the same
  `usage_digest` and `usage_confidence`, and the cost identity to be this attempt. A receipt whose
  usage came from another route, another attempt or another observation is refused before it can be
  stored in `FallbackReceiptLedger`.
- Fixtures: domain 6 → 15 deny-first tests (`kiana-domain/tests/bq18_fallback_admission.rs`),
  provider boundary 2 source guards plus one real behaviour test that builds an admission through
  the domain helper and asserts every route/credential/permit drift code
  (`kiana-provider/tests/bq18_fallback_admission.rs`), core source guard plus ControlPlane facade
  behaviour fixtures (`kiana-core/tests/bq18_fallback_admission_guard.rs`).
- CI wiring: manual `.github/workflows/bq18-fallback-admission.yml`
  (`workflow_dispatch`-only, `permissions: contents: read`, `cargo fetch --locked`, fmt gate, six
  serial targets, six `--no-run` compile steps). Automatic coverage stays with the unified `ci.yml`
  shards `kiana-domain-s1/4` (`bq18_fallback_admission`), `kiana-core-s1/6`
  (`bq18_fallback_admission_guard`) and the whole-crate `kiana-provider` shard; `scripts/ci/test-shards.json`
  and the Cargo manifests are unchanged.

Known limits: the allowlist, admission and ledger remain in-process value contracts. There is no
durable cross-process authority/CAS or allowlist snapshot store, no EventLog append/flush of
fallback admissions, no provider credential validity check, no invoice reconciliation, and no
live/physical fallback evidence. The provider validator is still exported without a production call
site, and `ControlPlane::re_admit_model_fallback` is a pure wrapper. Local Cargo tests, builds,
checks, clippy and smoke commands were intentionally not run in this session; the CI receipt is not
awaited and the proof level stays at `source`.
