# DEP-13 · Unified shutdown baseline

> Snapshot date: 2026-09-27. This slice defines a pure shutdown acknowledgement reducer over
> cancellation, fencing, drain and flush facts. Local Cargo test/build/check/clippy/smoke commands
> are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-13`](../roadmap.md#step-dep-13) |
| source snapshot | `1ca3be00` plus this DEP-13 source slice |
| feature_status | `partial` for shutdown decision/ack contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | typed cancellation/fence/drain/flush facts → `DaemonHost` route → Core shutdown report |

`ShutdownReport::Stopped` requires cancellation and intake pause, scheduler fence, runner/tool
drain, EventStore/artifact flush and EventStore close acknowledgements, with no active, unknown or
late-result work. Missing/failed acks produce `NeedsRecovery`; an explicit Unknown phase produces
`Unknown`. The reducer never infers stop from task drop or a process signal.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| normal | all eight phase facts and boundary acks produce Stopped |
| flush/drain | active work, missing flush, late result or expired deadline never produce Stopped |
| unknown | Unknown phase remains Unknown and requires reconciliation |
| completeness | missing/out-of-order/duplicate phase evidence is rejected or recovery-required |
| boundary | domain/Core/daemon paths do not execute cancellation, scheduler, process, EventStore or Broker effects |

## CI and limitations

GitHub Actions runs `.github/workflows/dep13-shutdown.yml` with domain fixtures, Core/daemon source
guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

This slice consumes adapter-supplied acknowledgement facts and does not cancel a runner, fence a
real queue, flush an EventStore/artifact, stop a process, persist a shutdown receipt or wire the
decision into ControlPlane command admission. Those effects and durable recovery remain later
adapter work.
