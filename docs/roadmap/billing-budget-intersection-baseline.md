# BQ-06 Budget Intersection Baseline

## Scope

BQ-06 adds a pure intersection over the existing `RuntimeBudget`, `ProjectBudget`, `Quota` and
`BudgetLease` plus the new `ProviderBudget`. `EffectiveBudget` takes the conservative minimum for
model calls, tokens, wall time and concurrency, carries project-run and provider token/cost caps,
and remains a digest-bound admission snapshot. `derive_child_lease` rejects any child widening or
usage reset instead of silently unioning limits.

FinancialBudget is intentionally absent: money/contract capacity cannot grant execution authority.
The calculation does not reserve, dispatch, settle, persist or call a provider; BQ-07 owns quota
windows and BQ-08 owns durable reservations/CAS.

## Evidence and limits

- `kiana-domain/tests/bq06_budget_intersection.rs` covers five-scope minimums, child widening,
  invalid provider/quota and empty-intersection denial.
- `kiana-core/tests/bq06_budget_intersection_guard.rs` protects the pure/no-financial-authority
  boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable reservation, quota
window, lease fence, queue, provider capacity or financial billing authority is claimed.
