# INT-27 connector notification baseline

> Snapshot date: 2026-09-27. This slice registers connector health/invocation/reconciliation/
> approval notification event kinds and validates their source/evidence/dedupe projection payload
> over the existing notification materializer. Local Cargo test/build/check/clippy/smoke commands
> are intentionally not run; GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-27`](../roadmap/integrations-connectors.md#step-int-27) |
| source snapshot | `a0de7993` plus this INT-27 source slice |
| feature_status | `partial` for connector notification registration and projection payload fences |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed connector event → registered notification class → source/evidence/dedup payload gate → existing NotificationMaterializer/HumanInbox |

The registry now recognizes `connector.health`, `connector.invocation`,
`connector.reconciliation` and `connector.approval`. `validate_connector_notification_payload`
requires connector/binding identity, committed source cursor, evidence digest, dedup key,
redacted summary and explicit limitation. It rejects model/UI sources, unregistered connector
families and secret-bearing text. Existing NotificationMaterializer remains the only projection and
its source cursor/replay/dedup behavior is reused.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| registry | all four connector notification classes are server-registered |
| source fence | model/UI or unknown connector event cannot materialize |
| evidence fence | source cursor/evidence digest/limitation/dedup key are mandatory |
| secret fence | summary/limitation are passed through redaction sentinel checks |
| projection boundary | no delivery, approval, HumanTask mutation or EventLog write in validation layer |

## CI and limitations

GitHub Actions runs `.github/workflows/int27-notifications.yml` with domain/Core fixtures, source
guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

Limitations: no notification delivery worker, durable outbox write, connector event producer,
per-channel policy decision or UI integration is added; existing materializer integration and
cross-entrypoint display remain source-level/unproven; INT-28+ remains open.
