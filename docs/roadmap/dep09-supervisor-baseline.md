# DEP-09 · SupervisorPort baseline

> Snapshot date: 2026-09-27. This slice defines a lease/fence-bound supervisor port and
> platform adapter boundary for systemd, launchd, Windows services and containers. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns fixtures
> and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-09`](../roadmap.md#step-dep-09) |
| source snapshot | `22671852` plus this DEP-09 source slice |
| feature_status | `partial` for the supervisor port and observation contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | ControlPlane-authorized operation lease/fence → `SupervisorPort` → platform adapter → typed observation |

`SupervisorRequest` uses a server-owned service reference with backend-specific prefixes and binds
operation/instance identity, deployment revision/root identity, lease digest, fence token,
authority/data epochs, expected generation and deadline. It rejects pid-shaped targets.
`SupervisorObservation` binds the request digest and requires confirmed `StopReport` evidence for
stopped/restarted success; a timeout, unknown result or unobserved forced kill cannot be reported as
success.

The port enumerates systemd, launchd, Windows service, container and fake backends. The daemon
`NarrowSupervisorAdapter` validates the binding and returns an explicit unavailable result until a
platform implementation can provide real signal and observation evidence.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| backend/reference | backend-specific target prefix is required; pid-shaped target is rejected |
| lease/fence | request digest, lease/root/revision digest, fence token, authority/data epoch and operation identity are immutable |
| stop | TERM/KILL without confirmed process-group observation cannot produce `Stopped` |
| restart | `Restarted` requires a strictly newer generation and confirmed stop evidence |
| unknown/timeout | unknown or timed-out outcomes remain explicit and cannot be treated as success |
| adapter boundary | daemon adapter has no model, capability, shell, pid fallback or direct process command |

## CI and limitations

GitHub Actions runs `.github/workflows/dep09-supervisor.yml` with domain fixtures, Core/daemon
source guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

This slice does not invoke systemd/launchd/Windows/container control, send OS signals, acquire an
operation lease, persist observations or wire a DaemonHost shutdown/startup coordinator. The
daemon adapter remains an explicit unavailable boundary; physical signal/timeout evidence and
durable CAS wiring are later deployment work.
