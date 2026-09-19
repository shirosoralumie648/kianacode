# CM-16 wire budget and stable prefix baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-16`](context-memory.md#step-cm-16) |
| feature_status | `implemented` (final-wire accounting modes and revocation-bound cache key) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | final serialized wire bytes + framing/output reserve → `WireBudget` → `StablePrefix` cache binding |
| authority | budget/cache identity is derived from route/config/prompt/tool/authority/data bindings; dynamic suffixes do not widen reuse |

## Contract and behavior

`WireBudget::from_final_wire` accounts the final serialized provider body once, with separate
output reserve/cap and context limit. `TokenAccounting::ExactTokenizer` binds exact counts to a
tokenizer digest; `ConservativeUtf8` remains an explicit upper-bound mode with safety margin.
Overflow, invalid accounting and total reserve over the declared limit fail closed.

`StablePrefix` now includes configuration revision and authority epoch in its cache key. Its
`cache_hit_allowed` check rejects prompt/tool/config changes, authority revocation and data-epoch
changes while continuing to exclude dynamic suffix content from the stable prefix identity.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `provider_request_never_exceeds_declared_budget` | exact tokenizer-bound final wire budget succeeds only below the declared total reserve |
| `cache_hit_cannot_bypass_revocation` | authority epoch and configuration revision changes reject a prior prefix cache hit |
| `wire_budget_and_cache_binding_stay_in_domain_contracts` | Core source guard keeps budget/cache logic in the domain contract without a second execution path |

## Proof ceiling and handoff

The CM-16 ceiling is `source` plus remote CI wiring. The repository still does not claim a live
provider tokenizer artifact, provider-side cache observability or durable cache recovery. CM-17
must bind this budget and prefix identity into one immutable `ResolvedStepContext`/prepared request.
