# DEP-18 - Incident schema, runbook evidence and alert routing baseline

> Snapshot date: 2026-09-27. This slice defines the incident lifecycle, runbook references,
> runbook evidence and alert-route metadata contract. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-18`](../roadmap.md#step-dep-18) |
| source snapshot | `08552ada` plus this DEP-18 incident source slice |
| feature_status | `partial` for source-level incident/runbook contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | server-owned incident facts -> bounded Core reducer -> DaemonHost read-only route |

`IncidentPhase` fixes the ordered lifecycle
`observed -> triaged -> contained -> recovering -> verified -> closed`. `IncidentInput` binds the
incident to an `OperationId`, source cursor, authority/data epoch, a `RunbookRef` for the requested
phase, an `AlertRoute`, ordered `history` and the current `RunbookEvidence`. `IncidentReport`
reduces that to a replayable decision.

Alert routes are metadata only. The report is structurally unable to express authority expansion or
automatic compensation: `authority_expansion` and `auto_compensation` are hard-coded `false`, the
report is rejected if either is ever set true, and `alert_route_read_only` must stay true.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| replayable | the six phases advance in order and an identical request replays to `Replayed` |
| phase order | skipping a phase, repeating a phase, reusing a source cursor or crossing the history bound is rejected |
| close gate | closing without a prior `Verified` evidence record is `Blocked` (`incident_close_requires_verified`) |
| unknown | unknown evidence, or unknown evidence anywhere in history, stays `Unknown` and requires reconciliation |
| deadline | an observation at or after its phase deadline is `Unknown` (`incident_phase_deadline_expired`) |
| binding | operation, revision, actor, cursor, epoch, runbook digest and alert-route digest must all match; tampering fails closed |
| safety | paths, `file://`, unredacted text and secret sentinels are rejected in every free-text field |
| boundary | Core/Daemon only reduce and delegate; no EventLog, Broker, provider, filesystem or process effect |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate, which compiles and
tests the affected targets. This slice adds no separate workflow: its guards reference only Rust
source, so a per-step workflow would have been another manual duplicate of the unified gate.

This is a source-only reducer over adapter-supplied facts. It does not open incidents, page
on-call, send alerts, schedule remediation, compensate, persist incident records, read the EventLog,
or observe real deadlines and clocks. Incident and evidence facts remain the later adapters'
authority. `Unknown` and `Blocked` remain non-advancing and require reconciliation; no phase is
promoted past `verified` without recorded evidence.
