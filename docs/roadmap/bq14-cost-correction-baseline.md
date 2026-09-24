# BQ-14 append-only CostLedgerEntry and CostCorrection baseline

> Snapshot date: 2026-09-25. This slice adds a bounded domain/core/query source contract and
> GitHub-only fixtures. Local Cargo tests, builds, checks, clippy and smoke commands are not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-14`](../roadmap.md#step-bq-14) |
| source snapshot | `e9e2d4ad` plus this BQ-14 source slice |
| feature_status | `implemented` for append-only ledger facts, approval-bound correction admission and read-only projection |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | BQ-13 cost fact → immutable CostLedgerEntry → versioned CostCorrection command → ControlPlane approval → EventLog append → query fold |

`CostLedgerEntry` carries the original amount, estimate/measured provenance, source event, source
cursor, revision and digest. `CostCorrectionCommand` has no transcript or model text field and
requires bounded evidence references, an exact target digest/run/attempt/cursor/revision fence and
an approval whose subject is the command digest. `CostCorrection` is a new fact; the reducer exposes
only `append_entry` and `append_correction`, preserving the original entry on replay. Replaying the
same command returns the original correction, while a changed digest or stale target is rejected.

The core helper validates the typed command and builds an immutable `cost.corrected` event without
dispatching a provider or capability. Query rebuilds a ledger from committed `cost.ledger_entry`
and `cost.corrected` events and computes a derived view without writing facts.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| missing approval/evidence | unapproved or evidence-free correction is rejected |
| target fence | run/attempt/source cursor/revision/digest drift is rejected |
| append-only | original entry remains byte-identical; no update/delete API exists |
| approval binding | approval command/target digest and independent approver are required |
| replay | same command digest/idempotency key replays one correction; conflicting digest fails |
| model boundary | typed command has no model text/transcript authority and core guard has no runner/broker/network path |
| projector | source event duplicate/sequence/run/stream mismatch fails closed; corrected totals are deterministic |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the BQ-14 domain/core/query
fixtures and source guards, and `cargo check --workspace --tests --locked`. Local Cargo tests,
builds, checks, clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: the reducer and event builders are source contracts and do not append/flush EventLog,
consume approval store state, provide cross-process CAS, import provider invoices, allocate
financial budgets or prove external billing. Approval consumption remains the existing
ControlPlane/ApprovalStore boundary, and daily rollups/projector checkpoints remain BQ-20.
