# INT-32 connector write pilot baseline

> Snapshot date: 2026-09-27. This slice adds a per-operation controlled write pilot gate. Local
> Cargo test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns the
> fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-32`](../roadmap/integrations-connectors.md#step-int-32) |
| source snapshot | `de7de928` plus this INT-32 source slice |
| feature_status | `partial` for independent write-operation evidence gate |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | per-operation payload/idempotency → approval/permit/receipt/cancel/compensation/reconcile evidence gate |

`ConnectorWritePilotGate` never treats an approval or receipt as a blanket connector grant. A ready
gate requires an isolated account, non-health operation, explicit approval ref, permit digest,
idempotency policy, final payload digest, provider receipt, cancellation fence, compensation plan,
reconciliation case, authority/data epochs, one-operation request bound and limitations. Blocked
gates remain harmless and this module contains no network/Broker/EventLog path.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| approval | missing or malformed approval blocks ready gate |
| permit/idempotency | independent permit and idempotency evidence are mandatory |
| outcome | provider receipt, cancellation, compensation and reconciliation refs are all required |
| exactly bounded | repeated request_count and health operation are rejected |
| isolation | connector identity cannot equal isolated account |
| effect boundary | source/Core gate cannot invoke provider, payment/refund, Broker or EventLog |

## CI and limitations

GitHub Actions runs `.github/workflows/int32-write-pilot.yml` with domain/Core fixtures, source
guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

Limitations: no write operation, provider idempotency API, receipt query, cancellation worker,
compensation execution or external reconciliation is run; this is not live/physical evidence;
INT-33 remains open.
