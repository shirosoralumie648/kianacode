# DEP-16 - Reconcile and repair baseline

> Snapshot date: 2026-09-27. This slice defines a read-only projector/index/queue/lease repair
> contract and an explicit `ops reconcile` decision. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-16`](../roadmap.md#step-dep-16) |
| source snapshot | `5fc439f5` plus this DEP-16 source slice |
| feature_status | `partial` for read-only repair/reconcile contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed source/projection/lease facts -> Core reconcile reducer -> DaemonHost read-only route |

`RepairFact` covers projector, index, queue and lease in one ordered set. A missing projection
cursor, `result_unknown`, source/projection cursor inversion, stale generation, inactive lease or
invalid fence never becomes healthy. `inspect` and `plan_repair` only report a next projection
generation; `commit_repair` requires an actor, evidence and an approval digest and returns
`commit_ready` without applying the repair.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| healthy | all four targets at the expected cursor/generation produce a read-only healthy report |
| plan/commit | lagging facts produce a deterministic next generation; explicit commit is approval-gated |
| unknown | missing cursor or result-unknown facts stay Unknown and never auto-rerun |
| fences | cursor-ahead, stale generation, inactive lease or invalid fence are Blocked |
| binding | target order, source/evidence/epoch/cursor drift, secret/path actor and digest tamper fail closed |
| boundary | Core/Daemon only reduce/delegate; no EventLog append, projection store, queue lease, broker or provider effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep16-reconcile.yml` with domain fixtures, Core and Daemon
source guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

This remains a source-only reducer over adapter-supplied facts. It does not read or rebuild an
EventLog, inspect a real `ProjectionCheckpoint`/index manifest/queue lease, persist a new
projection generation, execute an approval, or mutate a lease. The approval digest and source,
lease and evidence digests are opaque shape-checked bindings until later durable ControlPlane and
adapter work. `commit_ready` is not a committed repair receipt; Unknown reconciliation remains
explicitly deferred.
