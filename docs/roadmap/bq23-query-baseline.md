# BQ-23 billing query and protocol baseline

> Snapshot date: 2026-09-27. This slice adds one versioned read-only billing query envelope and
> a query adapter over the BQ-20 projection. Local Cargo test/build/check/clippy/smoke commands are
> intentionally not run; GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-23`](../roadmap.md#step-bq-23) |
| source snapshot | `acab8db9` plus this BQ-23 source slice |
| feature_status | `partial` for bounded budget/usage/export/reconciliation query contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | versioned protocol BillingQuery → boundary/cursor validation → BQ-20 read projection → common response |

`BillingQueryRequest` binds a project identity, server scope digest, source cursor, page limit and
read-only marker. A `BillingQueryCursor` binds the same boundary and projection digest, so a page
from another scope or projection cannot be consumed. `BillingQueryResponse` carries source cursor,
projection digest, freshness, Unknown/quarantine counts and an optional export digest.

`kiana-query::query_billing_projection` is the common adapter for budget summary, usage breakdown,
cost export and reconciliation inbox views. It reads `BillingProjectionSnapshot` only. Export needs
an explicitly allowed query data boundary; revoked/index-denied scopes fail before a response is
materialized. The protocol envelope has no reservation or approval mutation branch. Daemon ingress
validates identity and currently returns an explicit `billing_query_projection_not_wired` boundary
until the query projection is attached to the existing ControlPlane read surface.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| wire shape | all four kinds round-trip through one versioned `BillingQuery` body |
| boundary fence | wrong scope, revoked index or export-disabled boundary is denied |
| freshness | current, stale and unknown request cursors remain visible in the response |
| cursor fence | foreign projection/boundary and out-of-range page cursors fail closed |
| response shape | Unknown/quarantine counts, export digest and unknown fields are bounded |
| read-only guard | query adapter contains no EventLog append, reservation release or approval consumption |

## CI and limitations

GitHub Actions runs `.github/workflows/bq23-query.yml` with domain/protocol/query fixtures, source
guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

Limitations: the adapter returns bounded projection summaries rather than a durable page store or
per-attempt export materializer; DaemonHost currently rejects after ingress validation until a
ControlPlane projection method is attached. Server-side project-to-boundary authorization,
CLI/Web/Workbench projection routing, persisted query cursors, approval inbox records and
live/physical billing truth remain outside this source-only slice.
