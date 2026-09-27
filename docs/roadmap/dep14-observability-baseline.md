# DEP-14 · Observability evidence baseline

> Snapshot date: 2026-09-27. This slice defines operation/revision/cursor-bound metrics, trace
> refs, structured log and audit evidence metadata. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-14`](../roadmap.md#step-dep-14) |
| source snapshot | `f834a9e8` plus this DEP-14 source slice |
| feature_status | `partial` for redacted observability evidence contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed operation facts → typed evidence bundle → read-only Core/Daemon projection |

`LifecycleEvidenceBundle` binds every metric to one operation reference, revision digest and source
cursor, while trace/log/audit refs remain opaque and bounded. Duplicate metrics, cross-operation
or cross-cursor facts, secret markers, unknown fields and digest tamper are rejected. The contract
does not emit telemetry or append audit events; it prevents an unbound evidence-shaped bundle from
being presented as lifecycle proof.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| binding | operation/revision/source cursor drift is rejected |
| secret | trace refs containing secret/token markers are rejected; bundle remains secret-free |
| metrics | duplicate names and malformed/unknown metric facts fail closed |
| integrity | metric/bundle digest tamper fails closed |
| boundary | Core/Daemon paths only validate facts; no EventLog/Broker/provider/exporter effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep14-observability.yml` with domain fixtures, Core/daemon
source guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

This slice does not record live metrics, write structured logs, append audit events, export traces,
read EventLog or prove that a metric corresponds to a real external effect. Existing observability,
metrics and audit projectors remain the facts authorities; durable telemetry and cross-surface
status wiring are later work.
