# DEP-26 retention, deletion and revocation wiring baseline

> Snapshot date: 2026-09-28. Local Cargo **tests were not executed**; GitHub Actions owns fixtures
> and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-26`](#step-dep-26) |
| code landing | `kiana-core/src/retention_deletion.rs`, registered by `kiana-core/src/lib.rs` |
| fixtures | `kiana-core/tests/dep26_retention_deletion.rs`, `kiana-core/tests/dep26_retention_deletion_guard.rs` |
| feature_status | `partial` — the plan is decidable in source; nothing is deleted and no commit receipt exists |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## The four failures the card names, and the rule behind each

| Card failure | Refusal | Why |
|---|---|---|
| legal hold 下删除 | `retention_deletion_legal_hold` | A hold is the only reason in this slice that comes from outside the system. A demanded deletion of a held object is refused, not deferred. |
| 只删 projection 不留事实 | `retention_deletion_facts_missing` | The tombstone **is** the fact. If it goes first, the layer that was supposed to stop serving the payload has nothing left to be tombstoned against. |
| artifact/backup 引用悬空 | `retention_deletion_dangling_reference` | A retained object pointing at something that no longer exists is a dangling reference, and retention is the thing that made it possible. |
| revocation 不传播 | `retention_revocation_missing`, `..._incomplete`, `..._epoch_stale`, `..._receipt_missing` | Four different ways to not have propagated, kept apart because they have different fixes: nobody reported, a layer still answers, a layer is on an older epoch (which is exactly how a revoked object returns after a restore), or acknowledgement arrived without a receipt. |

## Ordering is imposed, not inherited

```text
Facts → Artifact → Memory → Index → Cache → Checkpoint
```

Targets are sorted into this order by the plan rather than accepted in the order the caller listed
them, because the caller's order is a wish and this is a constraint. A bounded plan that touched a
derived layer also sets `rebuild_required`, so "delete then forget to rebuild" is visible in the
plan instead of discovered later.

## A dry run that names deletions is not a dry run

In `DeletionMode::DryRun` the deletable set is emptied and every target moves to `protected` with
reason `dry_run`, while `deletion_order` still states what a real run would do. A dry run whose
output can be mistaken for a work list is worse than no dry run, because somebody will eventually
run it.

## Reused, not forked

`RevocationLayer`, `RevocationLayerState` and `RevocationLayerObservation` come from PD-26;
`BackupLegalHold` and `DeletionMode` come from the DEP-21 backup lifecycle. The guard asserts both
that those names are present and that the module does **not** define a parallel
`RetentionLayer`/`RetentionHold`/`RetentionDeletionMode` — a second layer enum would be a second
answer to "which layers are downstream of the facts".

## Plans, not deletions

The module produces a `RetentionDeletionPlan` with dependencies, a dry-run mode, protection reasons
and a seal. It deletes nothing, and the guard asserts the absence of every token that could:
`std::fs`, `remove_file`, `remove_dir`, `Command`, `EventStore`, `append_event` and `ControlPlane`.

## Honest limitations

This is a plan, not a deletion and not a commit. There is **no commit receipt** and no rebuild
verification in this slice — the card asks for both, and the plan is the part that could be decided
honestly without a store. Legal-hold state, per-layer propagation observations and retained
references are all **supplied by the caller**: nothing here reads a hold register, queries an
adapter or inspects an index, so a caller that under-reports any of them defeats the check. No
object was deleted, no artifact store or backup store was touched, no index was rebuilt, and the
plan is not wired into `ControlPlane::handle_command`. Retaining an object is also not implemented
here — the plan refuses to create a dangling reference, it does not resolve one that already
exists.
