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

## 2026-10-13 Startup Unknown preservation

The startup reducer now preserves an explicit `StartupCoordinatorStatus::Unknown` as
`SignalStatus::Unknown`, keeping `admission_allowed=false`. The existing `Ready` → `Ok` and
`Blocked` → `Degraded` mappings remain unchanged. This is a narrow reducer correction; it does
not claim that the upstream coordinator produced a trustworthy fact.

```text
source_snapshot: current dirty worktree; kiana-domain/src/health_aggregation.rs; kiana-domain/tests/dep11_health_aggregation.rs; kiana-core/tests/dep11_health_aggregation_guard.rs; kiana-daemon/tests/dep11_health_route_guard.rs
worktree_status: DEP-11 row 616 remains 🔄 / feature_status=partial / proof_level=source; selected startup Unknown mapping is local_behavior; guard source matching uses the existing lexical production view so explanatory comments do not satisfy effect markers
command_argv: bounded serial RED/GREEN exact selector `startup_unknown_is_preserved_as_unknown_and_never_ready`; full domain `dep11_health_aggregation`; Core `dep11_health_aggregation_guard`; daemon `dep11_health_route_guard`; all locked/offline with one Cargo worker and one Rust test thread
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile, locked/offline dependencies
fixture·cassette: valid Startup input with `StartupCoordinatorStatus::Unknown` and recomputed input digest; old reducer returned `Degraded`, corrected reducer returns `Unknown` and denies admission; domain 6/6, Core guard 3/3, daemon guard 3/3
exit_code: RED 101 / max RSS 4093768 KiB / CPU 92% / swap 0; exact GREEN 0 / max RSS 4099324 KiB / CPU 93% / swap 0; domain 0 / max RSS 83788 KiB / CPU 39% / swap 0; Core guard 0 / max RSS 139256 KiB / CPU 42% / swap 0; daemon guard 0 / max RSS 136812 KiB / CPU 53% / swap 0
status_change: Startup Unknown no longer collapses into ordinary Degraded; no readiness/liveness/drain/maintenance precedence expansion
proof-level_change: selected in-memory startup status behavior source → local_behavior; whole DEP-11 remains partial/source
limitations: no upstream StartupCoordinator proof, durable health checkpoint, live provider/exporter probe, admission wiring or cross-process recovery; other probe modes retain existing precedence
reviewer: root integration agent with DEP-11 read-only audit; focused health aggregation reviewer
```
