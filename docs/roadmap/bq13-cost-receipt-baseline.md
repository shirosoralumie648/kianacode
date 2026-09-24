# BQ-13 estimated/measured cost and Receipt breakdown baseline

> Snapshot date: 2026-09-25. This slice adds a bounded source contract and read-only receipt/query
> projections. Runtime fixtures run in GitHub Actions; no local tests are executed.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-13`](../roadmap.md#step-bq-13) |
| source snapshot | `2d31a8b0` plus this BQ-13 source slice |
| feature_status | `implemented` for cost breakdown contracts, Receipt aggregation wiring and query projector |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | normalized usage → pinned estimate or provider measured receipt → Receipt/query breakdown |

`CostBreakdown::from_usage` keeps absent/partial usage and missing price dimensions explicit. A
known estimate carries the exact `RateCardId` and `card_version` and exposes checked per-dimension
lines. `from_provider_receipt` rejects incomplete usage, missing RateCard scope, currency drift and
invalid opaque receipts; the serialized measured state contains only the provider receipt and
amount, never a rate-card estimate. Unknown retains a `BillingUnknownReason` and never writes a
numeric zero.

`ReceiptCostBreakdown` folds per-attempt facts into separate estimated/measured totals and a stable
unknown-reason list. Estimate totals are projection data only and cannot authorize execution or a
financial budget. Replayed estimate→measured facts replace the estimate for one attempt so a
provider receipt is not double-counted. Cross-run entries, duplicate source events, stream gaps,
kind drift and invalid transitions fail closed.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| pinned estimate | amount uses checked integer micros, has rate-card id/version and dimension lines |
| missing card / partial usage | state is Unknown with a reason and no amount; no cost=0 fallback |
| measured admission | incomplete usage or absent RateCard is rejected; measured carries opaque provider receipt |
| receipt fold | estimated and measured totals remain separate; unknown reasons stay visible |
| replay transition | one attempt's estimate→measured transition counts only the measured amount |
| identity/source fence | cross-run entries, duplicate events, stream gaps and malformed cost payloads fail closed |
| core Receipt | existing aggregation keeps legacy fields and adds optional typed `cost_breakdown` |
| query projector | source cursor/event IDs and unknown limitations are rebuilt read-only from committed facts |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain/core/query BQ-13
fixtures, the source guard and `cargo check --workspace --tests --locked`. Local tests, builds,
checks, clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: the contracts and projectors are read-only source projections. They do not append or
flush EventLog facts, consume permits, release quota reservations, import invoices, apply
corrections, reconcile external provider accounts or provide cross-process checkpoints. Financial
allocation, append-only ledger corrections and durable daily rollups remain BQ-14/BQ-19/BQ-20;
there is no provider billing, live or physical effect proof here.
