# BQ-19 project / organization / workflow / cell / run cost allocation baseline

> Snapshot date: 2026-09-25. This source slice adds one leaf allocation contract and deterministic
> read-only dimension projections. GitHub Actions owns runtime fixtures; local Cargo
> test/build/check/clippy/smoke commands are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-19`](../roadmap.md#step-bq-19) |
| source snapshot | `11aa9087` plus this BQ-19 source slice |
| feature_status | `implemented` for server-bound leaf allocation, SharingGrant admission and read-only rollup |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed BQ-13/BQ-14/BQ-15 leaf → ControlPlane allocation admission → `cost.allocation` fact → query projection |

`CostAllocation` binds one `UsageId` to server-owned organization/project/workflow/cell/run IDs,
source event/cursor/digest and revision. The dimensions are labels on that one leaf; the projector
folds each leaf once into independent run, cell, workflow, project and organization views. A repeated
usage leaf, duplicate source event, sequence regression or stream identity drift fails closed.
Estimated, measured and unknown cost states remain mutually exclusive. Measured requires an opaque
provider receipt and unknown never becomes zero.

A wire-reported scope is an assertion and must equal the trusted ControlPlane scope. Attribution to
a different project requires an exact active `SharingGrant` reference and the `cost.allocate`
operation; missing, expired, revoked, stale-epoch or mismatched grants are denied. No adapter or
projector treats a wire project as authorization.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| leaf uniqueness | two allocations for one `UsageId` fail; one leaf can appear in all dimension maps without multiplying totals |
| wire ownership | organization/project/workflow/cell/run drift is rejected before event construction |
| cross-project | no `SharingGrant`, wrong source/target/digest, expired/revoked or stale epoch is denied |
| source fence | event ID duplicate, sequence regression, run/allocation/revision mismatch fails closed |
| cost state | estimated is rate-card pinned, measured has provider receipt, unknown retains reason and no numeric zero |
| rollup | parent/child and multi-dimensional views are labels over the same leaf; ledger total is never the sum of dimension totals |
| read-only boundary | query rebuild has no EventLog/ledger mutation, authorization, reservation or execution loop |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the BQ-19 domain/core/query
fixtures and source guards, then `cargo check --workspace --tests --locked`. Local Cargo tests, builds,
checks, clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: allocation and rollup are immutable in-process source contracts and do not append or
flush EventLog facts, perform durable cross-process CAS, consume/release quota, validate external
provider/project truth, import invoices or authorize financial budgets. A `SharingGrant` is checked
against the supplied authority snapshot but grant persistence/consumption remains the existing
ControlPlane boundary. Parent/child workflow accounting, daily/window checkpoints and recovery
remain BQ-20/BQ-21 and later CompanyOS work.

## 2026-10-17 completion slice (CI-only)

The source slice above was kept; this pass closes the card's deny column and success column with
deny-first fixtures and a single-rule grant check.

| Added / hardened | Where | Why |
|---|---|---|
| `CostAllocationLedger` + `CostAllocationAppendOutcome` | `kiana-domain/src/billing_allocation.rs` | one usage leaf can be charged exactly once; duplicate allocation id, conflicting replay digest, reused source event and source-cursor regression fail closed. Replay of the identical fact returns the original leaf |
| `CostAllocation::validate_grant_binding` | domain | Domain owns the exact/active/`cost.allocate` grant rule; Core delegates instead of re-implementing it |
| `CostAllocation::validate_wire_attribution` | domain | wire scope, source project and grant reference are assertions compared field by field; mismatches fail before an event exists |
| negative amount fences | domain | `cost_allocation_estimated_amount_negative` / `cost_allocation_measured_amount_negative`; zero stays a legal explicit observation |
| `CostAllocationAdmission::authorize_wire_attribution`, `ControlPlane::admit_cost_allocation_wire` | `kiana-core/src/billing_allocation.rs` | the wire path reaches the same ControlPlane facade and error classification as the trusted path |
| recomputation in `CostAllocationProjection::validate` | `kiana-query/src/allocation_projector.rs` | hand-edited totals are refused (`cost_allocation_projection_totals_recompute_mismatch`) and duplicate `source_event_ids` are refused |
| `CostAllocationPortfolio` + `project_cost_allocation_portfolio` | query | CO-45 portfolio view is folded once from the same leaf set with a leaf-set digest; row leaf counts must equal the ledger total |

Fixture matrix after this slice (deny first): domain 8, core 4, query 8. Guards pin the markers,
refusal-code matrix, fixture names, the manual lane commands and the `scripts/ci/test-shards.json`
bindings (`kiana-domain-s1/4` lists `bq19_cost_allocation`; `kiana-core-s1/6` lists
`bq19_cost_allocation` and `bq19_cost_allocation_guard`; `kiana-query` is a whole-crate shard).

Manual lane: `.github/workflows/bq19-cost-allocation.yml` (`workflow_dispatch`-only,
`permissions: contents: read`, `cargo fetch --locked`, `cargo fmt --all --check`, five test targets
with `--test-threads=1` and five `--no-run` compile steps).

Known limits after the completion slice: local tests/build/check/clippy/smoke were not run and CI
receipts are not awaited, so the proof ceiling stays `source`. Allocation facts are still not
durably appended here; cross-process CAS, quota settlement, invoice import, daily/window rollups,
parent/child recovery and live/physical billing evidence remain BQ-20/BQ-21/BQ-22 and later.

