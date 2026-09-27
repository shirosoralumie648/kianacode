# DEP-21 - Incremental backup chains, retention and legal hold baseline

> Snapshot date: 2026-09-27. This slice owns the relationship between backups in a chain and the
> decision to delete them. Local Cargo test/build/check/clippy/smoke commands are intentionally not
> run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-21`](../roadmap.md#step-dep-21) |
| source snapshot | master plus this DEP-21 backup lifecycle slice |
| feature_status | `partial` for source-level chain/retention/hold contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | backup entries -> chain validation -> retention plan |

An incremental backup stores a delta against a parent and **cannot be restored without it**. That
single fact drives this slice. `BackupChainEntry` records one backup's link, state, parent,
covered/source cursors, generation, data epoch and encryption key reference. `BackupChain` validates
that the entries actually form a restorable chain.

## Deletion is a decision, not an action

`BackupDeletionPlan::plan` returns what retention *may* remove. It never removes anything. A
backup is refused when:

- **a legal hold covers it** — the refusal names the hold, so the plan is auditable against it;
- **a live or archived child still depends on it** — removing the parent would leave that child
  unrestorable, and that failure only surfaces at restore time, long after the evidence is gone.

A `DryRun` plan carries an empty removal list and is rejected by `validate_against` if one is ever
attached, so a report cannot be acted on by mistake. A `Bounded` plan orders removals child-first,
so the chain is never left pointing at a backup that has already been removed.

## What the card rejects, and how

| Rejected | How |
|---|---|
| incremental decided by mtime alone | an `Incremental` entry whose `covered_from_cursor` equals its `source_cursor` stores no new restore point and is rejected (`backup_chain_incremental_covers_nothing`); a full backup is explicitly allowed to cover its whole range |
| deleting a still-referenced parent | the parent of any live or archived child is in the `required` set and cannot be deleted (`backup_required_by_live_child`) |
| missing key reference | a `Full` backup may not declare a parent and an `Incremental` one must; a nil key reference makes an encrypted backup unrestorable (`backup_chain_encryption_key_missing`). Only the opaque `SecretRefId` travels with the entry, never key material |
| an ignored hold | a hold with no backups is rejected (`backup_legal_hold_header_invalid`) rather than silently protecting nothing; a duplicated backup under one hold is rejected; every refusal in a plan names its hold |

A chain also rejects a parent that does not precede its child, a parent whose `covered_from_cursor`
disagrees with the child that claims it (which would leave a gap no backup covers), duplicate
backup ids, and non-increasing source cursors.

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| restore order | a child restores only through its parent, oldest first |
| link shape | a full backup with a parent and an incremental without one are both rejected |
| mtime-only delta | an incremental that covers nothing new is rejected |
| chain gap | a child whose `covered_from` disagrees with its parent is rejected |
| dependent parent | an expired parent of a live child is retained, not deleted |
| legal hold | a held backup is retained and the refusal names the hold |
| unblocked deletion | an unreferenced, unheld expired backup is deletable; a dry run reports nothing |
| key reference | a nil key reference blocks restore; an unencrypted backup needs no key |
| terminal state | a deleted or unknown backup is never restorable |
| plan binding | a plan re-validated against a different chain is rejected |
| hold integrity | an empty or duplicated hold is rejected |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

This is a source-only planner over adapter-reported facts. It does not copy, delete, archive,
encrypt, decrypt, resolve a key reference, or touch a retention store. The deletion dependency
graph is derived from the entries supplied, so an adapter that omits a child makes a deletable
parent look safe; proving the chain against the real store is later work. The card also names
incremental copy, archive storage and retention execution as part of this step, and none of those
effects are implemented here — only the decision that would authorise them. Merging this with
PD-22 `SnapshotManifest` and PD-25 retention remains the follow-up recorded in the DEP-20 baseline.
