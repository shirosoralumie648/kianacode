# SW-16 directed swarm handoff baseline

> Snapshot date: 2026-09-27. This slice adds structured directed WorkPacket/HandoffAck/StatusReport/
> Evidence/Incident records and bounded Symposium metadata. Local Cargo test/build/check/clippy/
> smoke commands are intentionally not run; GitHub Actions owns fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SW-16`](../roadmap.md#step-sw-16) |
| source snapshot | `e127927b` plus this SW-16 source slice |
| feature_status | `partial` for directed handoff/status/evidence/incident source contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | named sender/recipient → structured record/ACK/evidence → ControlPlane re-entry for any action |

`SwarmHandoffRecord` requires a named recipient, packet ref, source cursor and redacted summary;
Evidence/Incident records require evidence refs, HandoffAck requires ack digest, and Symposium is
bounded to eight rounds. `authority_granted` is permanently false. No free broadcast or SendMessage
bus is introduced; Builder/monitoring attendance remains governed by existing role boundaries.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| directed recipient | broadcast/empty/unknown recipient shapes fail |
| ACK/evidence | HandoffAck/evidence/incident require their evidence fields |
| Unknown | NeedsReconciliation requires evidence and cannot be success |
| symposium | round/max bound and non-Symposium field leakage fail |
| authority | records never grant authority or carry effect execution |
| shape | digest/unknown fields fail closed |

## CI and limitations

GitHub Actions runs `.github/workflows/sw16-handoff.yml` with domain/Core fixtures, source guard
and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands are
intentionally not run, and CI results are not awaited.

Limitations: no UI/event projection, durable handoff delivery, WorkPacket scheduler integration,
Symposium runtime or ControlPlane mutation wiring is added; SW-17+ remains open.
