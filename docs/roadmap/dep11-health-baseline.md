# DEP-11 · Health aggregation baseline

> Snapshot date: 2026-09-27. This slice adds a conservative health aggregation contract over
> typed startup, operation, lease, projection and provider evidence. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns fixtures
> and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-11`](../roadmap.md#step-dep-11) |
| source snapshot | `c0b36b39` plus this DEP-11 source slice |
| feature_status | `partial` for health/readiness aggregation |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | typed adapter facts → `DaemonHost` health route → Core reducer → existing `HealthSnapshot` projection |

`HealthAggregationInput` binds source cursor/event IDs, startup report status/digest, operation
state/phase/deadline, active lease/fence/epochs, projector cursor, provider evidence and probe
mode. Readiness becomes `Ok` only when startup is Ready, projection is caught up, lease/fence and
epochs match, operation is known and not expired, and provider health has a verified evidence flag.
Provider self-report alone produces an Unknown provider component and keeps admission closed.
Unknown, lease conflict, projection lag/cursor-ahead, drain and maintenance remain visible in the
report; the report's `admission_allowed` is false unless the readiness status is actually `Ok`.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| provider | healthy without verified evidence remains Unknown and cannot make readiness Ready |
| projection | lag or cursor-ahead is Degraded and blocks admission |
| lease/epoch | inactive lease, invalid fence or authority/data epoch drift is Unknown and blocks admission |
| operation | Unknown state or expired deadline is preserved as Unknown |
| probes | startup/readiness/liveness/drain/maintenance have explicit status; drain and maintenance never admit |
| transport | HTTP status, provider calls, EventStore/Broker effects and second loops are outside this reducer |

## CI and limitations

GitHub Actions runs `.github/workflows/dep11-health.yml` with domain fixtures, Core/daemon source
guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

This slice consumes adapter-supplied typed digests and does not read live EventLog/provider state,
persist heartbeat or lease facts, validate the full StartupCoordinatorReport/OperationJournal/
OperationLease objects, enforce command admission, change HTTP status, or prove provider/Broker/
exporter liveness. Existing OA-11 health projection remains the EventLog projection authority;
durable wiring and ready admission are later deployment/health steps.
