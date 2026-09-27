# DEP-12 · Admission and drain baseline

> Snapshot date: 2026-09-27. This slice defines explicit ready, paused, maintenance, draining,
> blocked and Unknown admission decisions. Local Cargo test/build/check/clippy/smoke commands are
> intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-12`](../roadmap.md#step-dep-12) |
| source snapshot | `191c80ec` plus this DEP-12 source slice |
| feature_status | `partial` for admission decision contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | typed Health/Startup/Lease/Operation evidence → `DaemonHost` route → Core admission decision |

`MaintenanceWindow` is immutable and digest-bound. The health digest/status pair is an opaque,
adapter-supplied source fact in this source-only slice; it is not a live `HealthSnapshot` proof.
`DeploymentAdmissionDecision` separates
`allow_new_work` from `allow_existing_work`: pause, maintenance and active drain reject new work
while allowing already-started work to drain; an expired window never extends itself and an expired
drain becomes Unknown. Health/lease/epoch/operation Unknown and migration/backup overlap block
admission, while Ready requires healthy startup and matching lease epochs.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| maintenance | active window yields Maintenance; expiry yields explicit remediation and no automatic extension |
| pause/drain | Paused/Draining reject new work but preserve existing-work policy; expired drain becomes Unknown |
| ready conflict | migration or backup cannot coexist with Ready |
| health/lease | degraded/unknown health, inactive or epoch-drifted lease and Unknown operation fail closed |
| window integrity | tampered window/decision digest and missing drain deadline are rejected |
| boundary | domain/Core/daemon paths contain no EventStore/Broker/process/HTTP effect and no second loop |

## CI and limitations

GitHub Actions runs `.github/workflows/dep12-admission.yml` with domain fixtures, Core/daemon source
guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

This slice consumes typed evidence and does not mutate scheduler intake, pause a queue, stop an
operation, write EventLog, acquire a lease, change HTTP status or enforce command admission. Durable
ControlPlane wiring and unified shutdown remain later DEP-13+ work.
