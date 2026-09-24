# BQ-11 model attempt lifecycle baseline

> Snapshot date: 2026-09-25.  This slice adds a bounded, source-level lifecycle contract for
> admitted model attempts.  Runtime tests are GitHub-only and are not run locally.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-11`](../roadmap.md#step-bq-11) |
| source snapshot | `778cb7c3` plus this BQ-11 source slice |
| feature_status | `implemented` for the domain/core lifecycle contracts and CI fixtures |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `prepared` → flushed append acknowledgement → `dispatching` → `observed` → `settled` or `unknown` |

The lifecycle record binds server-owned run/turn/model-attempt identity to the billing `AttemptId`,
`QuotaReservation`, permit reference, `NormalizedUsage` and a receipt reference.  A pure ledger emits
immutable facts; the core projector folds only committed lifecycle facts and leaves legacy model
events readable without treating them as billing facts.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| unprepared dispatch | no `model.dispatching` fact can be emitted before `model.prepared` |
| permit fence | a missing or mismatched permit is rejected before dispatch |
| flush fence | an append without a successful prepared flush acknowledgement cannot dispatch |
| duplicate settlement | a terminal `settled` attempt cannot settle a second time |
| usage identity | usage from another run/attempt cannot be observed or settled |
| unknown retention | an uncertain provider result keeps usage when available and retains a bounded error |
| projector replay | the core read model requires the prepared first fact, checks revisions/transitions,
  and uses only RuntimeEvent source facts |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain lifecycle
fixtures, the core source guard and `cargo check --workspace --tests --locked`.  Local tests,
builds, checks, clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: the ledger is a pure in-process contract and does not itself append/flush an EventLog,
consume a provider permit, settle/release a quota reservation or reconcile an external provider
receipt.  The core projection is read-only and has no durable checkpoint, crash recovery, invoice
reconciliation, retry/fallback policy or live provider effect proof.  Legacy `model.prepared`
payloads remain owned by the existing budget path until a later migration; only payloads carrying
the BQ-11 schema enter this projector.
