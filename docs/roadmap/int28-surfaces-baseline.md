# INT-28 connector surface baseline

> Snapshot date: 2026-09-27. This slice adds one shared read-only connector query/response DTO
> for CLI, Web, Workbench and MCP plus a protocol re-export. Local Cargo test/build/check/clippy/
> smoke commands are intentionally not run; GitHub Actions owns the fixtures and target
> compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-28`](../roadmap/integrations-connectors.md#step-int-28) |
| source snapshot | `a847b210` plus this INT-28 source slice |
| feature_status | `partial` for shared read-only query/response/cursor contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | surface query DTO → scope/source cursor validation → common projection response; reconcile mutation remains ConnectorCommand route |

`ConnectorSurfaceQuery` requires a server-bound binding/scope digest, page limit, source cursor and
`read_only=true`; it has no actor, approval, lease or context override fields. `ConnectorSurface`
enumerates CLI/Web/Workbench/MCP and `ConnectorSurfaceQueryKind` covers health, invocation,
reconciliation and approval views. `ConnectorSurfaceResponse` carries projection digest,
Freshness, evidence digest, unknown status, limitations and a bound cursor. Summaries are redacted
and pagination cannot cross binding/scope/source identity.

The DTO is deliberately read-only. Manual reconcile still goes through the existing versioned
`ConnectorCommand::Reconcile` → DaemonHost/ControlPlane route rather than a UI-specific mutation.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| four-surface parity | all four surface enum values round-trip through one query shape |
| read-only fence | false read_only and unknown actor/approval fields fail closed |
| scope/cursor | foreign binding/scope/source/page cursor is rejected |
| response evidence | projection/freshness/limitation/evidence/Unknown fields are mandatory |
| secret fence | item title/summary and limitations reject redaction sentinels |
| route boundary | no surface DTO creates lease, approval, connector effect or EventLog write |

## CI and limitations

GitHub Actions runs `.github/workflows/int28-surfaces.yml` with domain/protocol/Core fixtures,
source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

Limitations: CLI/Web/Workbench/MCP handlers are not yet wired to a live common projection in this
slice; no durable cursor store, notification query backend, reconciliation UI action or live
connector evidence is claimed; INT-29+ remains open.
