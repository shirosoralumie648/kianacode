# DEP-22 - Restore quarantine root and admission baseline

> Snapshot date: 2026-09-27. This slice decides whether a restored root may be opened in
> quarantine before anyone considers activating it. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-22`](../roadmap.md#step-dep-22) |
| source snapshot | master plus this DEP-22 restore quarantine slice |
| feature_status | `partial` for source-level quarantine and admission contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | restored root facts -> structural admission -> ordered scan -> activation eligibility |

A restore must never write where the instance is running. `RestoreRoot` therefore carries both the
restored root and the root the instance is actually using, and `RestoreQuarantine` re-checks that
pair for every root it holds. Roots are identified by `StorageRootId`/`InstanceId`, never by a path.

This slice is the step *before* activation. PD-23's `verify_restore` already validates a sealed
manifest and a replacement-fence fact; DEP-22 owns the quarantine and the scan that must pass
first, and hands an eligible root to that later step rather than activating it itself.

## The stage ladder

`Staged -> Verified -> Rebuilt -> Ready`, with `Rejected` terminal. Two properties matter:

- **Only `Ready` is activation-eligible.** A root that has been scanned but not rebuilt is a
  directory, not a restore.
- **Stages never move backwards.** `advance` refuses a stage that is not strictly greater, so a
  scan cannot be undone by replaying an older decision, and a rejected root can never be revived.

`RestoreScanReport::activation_eligible` requires *both* a complete scan and a `Ready` root.
Either alone is insufficient.

## What the card rejects, and how

| Rejected | How |
|---|---|
| overwriting the active root | a root whose `storage_root` or `instance_id` equals the active one is refused structurally (`restore_root_would_overwrite_active`); the quarantine re-checks it, so a root admitted against one active root cannot be activated against another |
| hash/identity/generation/epoch regression | `projection_cursor > source_cursor` is a fabricated projector position (`restore_projection_cursor_ahead_of_source`); a data or authority epoch that does not strictly exceed the previous one is refused (`restore_data_epoch_not_advanced`, `restore_authority_epoch_not_advanced`), because a rewound epoch would let a write made under the old epoch be accepted again |
| missing artifact | the scan compares the manifest's artifact inventory against the inventory the adapter reports; anything absent is named in `missing_artifacts` and the root is `Rejected` |
| unknown / unhandled migration | a root carrying a `migration_version` that was not applied is `MigrationPending`; a root claiming `migration_applied` with no version to apply is equally incoherent (`restore_migration_version_missing`) |

The scan order is fixed, because a missing artifact makes the hash of that chunk meaningless, and
a schema this build cannot read has no migration path to speak of: missing artifact, then
migration, then complete.

`ResumeProjections` is refused without a projection cursor (`restore_resume_without_projection_cursor`):
resuming from a checkpoint that was not in the backup is not a resume, it is a guess.

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| stage ladder | verified is not eligible; only a ready root is |
| active root | a root sharing the active storage root is refused |
| projector position | a projection cursor past the source is refused |
| epoch regression | neither data nor authority epoch may stay or rewind |
| missing artifact | the absent reference is named and the root is rejected |
| pending migration | an unapplied migration blocks the restore; an applied one needs a version |
| terminal rejection | a rejected root cannot be advanced |
| monotonic stages | a stage cannot move backwards |
| mode coherence | resuming without a checkpoint is refused |
| report binding | a stale digest fails closed, and a disqualifying scan may not present as Ready |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

This is a source-only admission contract over adapter-reported facts. It does not copy a file,
create a directory, read a manifest, verify a signature, hash a chunk, rebuild a projector or
index, acquire a lease, or activate a root. The scan trusts the `present_artifacts` inventory the
adapter supplies, so an adapter that reports an artifact as present when it is not defeats the
check entirely; comparing real bytes is later work. The projector/index rebuild this card names is
represented as a stage transition and a mode, not implemented. Signature verification belongs with
DEP-34's release preflight, and activation itself to DEP-23.
