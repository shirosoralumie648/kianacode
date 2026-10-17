# BQ-07 Quota Window Baseline

## Scope

BQ-07 adds trusted-clock UTC quota windows, canonical provider/credential/model/alias group keys,
and bounded window-budget checks with retry-after evidence. Window creation rejects untrusted or
rolled-back `ClockObservation`; aliases and identifiers are normalized into one digest-bound group
key so casing/alias variants cannot bypass quota. Empty scopes, invalid limits and over-limit
requests fail closed.

The contracts are pure and in-memory. They do not reserve a durable slot, mutate usage, dispatch a
provider or claim financial authorization; BQ-08 owns durable reservation/fence/CAS.

## Evidence and limits

- `kiana-domain/tests/bq07_quota_window.rs` covers deterministic UTC windows, retry-after, clock
  rollback, alias canonicalization, empty group and over-limit behavior.
- `kiana-core/tests/bq07_quota_window_guard.rs` protects trusted-clock/no-I/O/no-provider boundary.
  GitHub Actions runs the fixtures. The original baseline predated the 2026-10-06 user request
  authorizing bounded local tests; the current focused receipt is below.

This slice is `feature_status=implemented`, `proof_level=source`: no durable reservation, fair
queue, lease fence, cross-process clock store or provider capacity backend is claimed.

## 2026-10-08 current-time retry-after hardening

`QuotaWindowBudget::check_at` now accepts a trusted `ClockObservation` and computes retry-after
from the observed wall time. A 120000..180000 ms window checked at 125000 ms therefore returns
55000 ms rather than the full 60000 ms. Rollback/untrusted observations are denied before quota
evaluation. The older `check` method remains as a compatibility API whose start-of-window delay
is intentionally unchanged.

```text
source_snapshot: current dirty worktree; kiana-domain/src/billing_quota.rs; kiana-domain/tests/bq07_quota_window.rs; kiana-core/tests/bq07_quota_window_guard.rs
worktree_status: BQ-07 row 196 remains 🔄 / feature_status=implemented / proof_level=source; selected current-time retry-after is locally verified; no durable reservation/CAS or provider capacity wiring
command_argv: RED `cargo test -p kiana-domain --test bq07_quota_window quota_window_budget_rejects_over_limit_with_retry_after --locked --offline -- --exact --test-threads=1`; GREEN domain `cargo test -p kiana-domain --test bq07_quota_window --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test bq07_quota_window_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; RED max RSS 223968 KiB / 42% CPU; domain GREEN max RSS 83080 KiB / 34% CPU; Core guard max RSS 85228 KiB / 35% CPU; all measured runs reported 0 swaps
fixture·cassette: RED legacy `check` returned 60000 ms at a 125000 ms observation; GREEN domain 4/4 asserts `check_at` returns 55000 ms and rejects untrusted/pre-window clock, while compatibility `check` remains 60000 ms; Core guard 1/1
exit_code: RED 101 expected; GREEN domain/Core 0
status_change: selected BQ-07 current-time retry-after and trusted-clock boundary moves source → local_behavior; whole card remains feature_status=implemented, proof_level=source
proof-level_change: exact retry-after calculation source → local_behavior; no durable/live/physical promotion
limitations: old `check` remains start-of-window compatibility behavior; durable reservation/fence/CAS, fair queue, cross-process clock store and provider capacity backend remain BQ-08+
reviewer: root integration agent with BQ-07 read-only quota audit; focused clock/retry reviewer
```

## 2026-10-06 focused fixture receipt

The over-limit fixture previously requested one unit when one request remained, so the budget
correctly accepted it. The fixture now requests two units and asserts the structured
`quota_exhausted:retry_after_ms=` denial before lowering `used_requests` and confirming the
allow case. The focused domain target passed 3/3 tests; the full workspace remains non-green,
and row 196 remains open. `QuotaWindowBudget::check()` currently derives retry-after from the
window start because the API has no current-time argument; this receipt proves presence of a
quota denial only, not an exact current-time retry-after calculation.

## 2026-10-17 exact boundaries and group aggregation

The card's deny/allow lists are now fully covered by `kiana-domain/tests/bq07_quota_window.rs`
(auto-run by `.github/workflows/bq07-quota-window.yml`, no new target):

- exact current-instant retry-after at window boundaries: `119999 -> 1`, `120000 -> 60000`,
  `179999 -> 1`, `180000 -> 0` (`quota_check_at_reports_exact_retry_after_at_window_boundaries`);
- rolling windows on exact aligned boundaries with no partial-second drift
  (`quota_window_rolls_on_exact_boundaries_without_partial_second_drift`);
- misaligned windows, zero/oversized duration and empty credential/model scopes failing closed
  (`misaligned_window_and_empty_group_scope_fail_closed`);
- alias/casing variants of the same provider/credential/model folding to one `group_digest`, so
  they share one bucket and cannot bypass an exhausted quota, while a different credential stays
  a distinct bucket (`alias_and_casing_variants_aggregate_into_one_quota_bucket`);
- a tampered `group_digest` is rejected before it can join the aggregation
  (`tampered_group_digest_is_rejected_before_aggregation`).

Aggregation is keyed by the canonical `group_digest`; the contract stays pure and in-memory.
Durable reservation, lease/fence/CAS, cross-process clock store and provider capacity remain
BQ-08+. Evidence is `feature_status=implemented`, `proof_level=source` (CI pending).
