# DEP-05 · Operation lifecycle baseline

> Snapshot date: 2026-09-27. This slice defines a bounded deployment operation state machine,
> phase deadlines and replayable operation journal. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-05`](../roadmap.md#step-dep-05) |
| source snapshot | `e2b50107` plus this DEP-05 source slice |
| feature_status | `partial` for the pure lifecycle/replay contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | EventStore facts → operation transition reducer → Core read-only projection → later lease/supervisor/health steps |

`kiana-domain/src/operation_lifecycle.rs` keeps operation state separate from
`DeploymentPhase` and run execution status. Every transition carries the operation and deployment
revision identity, monotonic source cursor, reason, evidence references, observed time and a
stable digest, and `kiana.operation-lifecycle.v1` is registered in the domain schema registry.
Preflight must complete before drain or execution; a drain deadline timeout folds
to `Unknown` and never to `Stopped`; terminal state is immutable.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| skip preflight | execute transition from `Created` is rejected |
| late revision | revision id/digest drift is rejected before projection |
| drain timeout | only a due drain deadline can produce `Unknown`; no `Stopped` state exists |
| duplicate terminal | a second terminal transition is rejected |
| cursor regression | source cursor cannot move backwards or repeat |
| replay | the same ordered transitions rebuild the same terminal journal and digest |
| source boundary | domain/Core contain no filesystem, process, Tokio, Broker, EventStore append or dispatch effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep05-operation-lifecycle.yml` with domain fixtures, a
Core source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and
smoke commands are intentionally not run, and CI results are not awaited.

This slice does not persist the journal, acquire an operation lease, fence a writer, invoke a
supervisor, mutate a deployment root, publish health, consume approval or dispatch a capability.
EventStore/JournalFrame remains the fact source; the reducer is a deterministic projection until
the later DEP-06+ slices provide durable lease, startup, health and shutdown integration.
