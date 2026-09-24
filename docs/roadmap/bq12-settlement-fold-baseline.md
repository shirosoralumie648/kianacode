# BQ-12 settlement/release/unknown fold baseline

> Snapshot date: 2026-09-25. This slice adds a bounded source contract and read-only projector.
> Runtime fixtures are GitHub-only and are not run locally.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-12`](../roadmap.md#step-bq-12) |
| source snapshot | `88959eb5` plus this BQ-12 source slice |
| feature_status | `implemented` for the domain fold contract, protocol/event registry, core projector and CI fixtures |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `reserved` → `consumed` → explicit `released`, or `reserved` → `unknown` → reconciliation |

The fold binds `RunId`, `ModelAttemptId`, billing `AttemptId`, `QuotaReservationId`, reservation
digest, optional normalized usage digest, receipt reference and source event references. Known
usage records only the measured consumption; an explicit release event can return the unused
reservation remainder. Partial usage keeps the fold occupied and is marked `partial`. Cancellation,
timeout, EOF and `result_unknown` remain `unknown` with `reconciliation_required=true`; they never
become an automatic zero or release.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| known settlement | consumed units are bounded by the reservation; only an explicit release returns the unused remainder |
| partial usage | known portion is retained and partial usage cannot be refunded before reconciliation |
| unknown result | reservation remains conservatively occupied and release is rejected |
| duplicate settlement/release | terminal transitions and conflicting digests cannot charge or release twice |
| identity/digest fence | cross-run, cross-attempt and reservation/usage source drift fail closed |
| core projector | only committed `usage.*` facts are folded; schema, kind, revision and source replay are checked |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain settlement
fixtures, the core source/projector guard and `cargo check --workspace --tests --locked`. Local
tests, builds, checks, clippy and smoke commands are intentionally not run; CI results are not
awaited.

Limitations: the ledger is a pure in-process contract and does not itself append/flush an
EventLog, consume a provider permit, mutate a quota adapter or reconcile an external receipt.
The projector is read-only and has no durable checkpoint, crash recovery, invoice correction,
retry policy, cross-process CAS or live provider effect proof. A partial or unknown fold therefore
remains occupied until a later explicit reconciliation step; no `durable`, `live` or `physical`
claim is made.
