# BQ-21 billing restart and recovery baseline

> Snapshot date: 2026-09-27. This slice adds a strict source fact for restart, continue,
> replay and recovery around an in-flight billing reservation and an uncertain model attempt.
> Local Cargo test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions
> owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-21`](../roadmap.md#step-bq-21) |
| source snapshot | `a9111a00` plus this BQ-21 source slice |
| feature_status | `partial` for the source recovery and fencing contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed recovery fact → Core validation facade → caller-owned reconciliation |

`BillingRecoveryFact` keeps the attempt and reservation digest, source cursor, authority epoch and
lease epoch together. Restarted work is paused or remains `NeedsReconciliation` while usage is
unknown. `Recovered` requires an explicit reconciliation reference, known usage and exactly one
settlement; paused/reconciliation states carry zero settlements. `Continue` cannot reset
accumulated usage. The epoch fields make an old lease visible to the caller so a stale worker
cannot be treated as the current authority by this contract.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| default recovery | paused and needs-reconciliation states remain distinct; unknown is visible |
| continue fence | a continue request that resets usage is rejected |
| success fence | unknown usage cannot be represented as recovered/automatic success |
| single-settlement fence | recovered state cannot claim more than one settlement |
| epoch fence | zero lease/authority epochs fail closed |
| digest/shape fence | digest tampering and unknown fields are rejected |
| read-only boundary | Core/domain source contains no provider dispatch, EventLog append or reservation release |

## CI and limitations

GitHub Actions runs `.github/workflows/bq21-recovery.yml` with formatting, the domain fixtures, the
Core source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and
smoke commands are intentionally not run, and CI results are not awaited.

Limitations: this is a caller-supplied source contract. It does not restart workers, rebuild an
EventLog projection, perform CAS/reconciliation, settle a reservation, or prove that an external
provider effect was known. Durable recovery and live/physical billing remain unproven.
