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
