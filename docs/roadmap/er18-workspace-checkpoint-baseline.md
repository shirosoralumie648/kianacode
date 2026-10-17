# ER-18 workspace checkpoint transaction baseline

## Scope

ER-18 binds workspace checkpoints to immutable file snapshots and the same ControlPlane restore
action: path/symlink/hardlink scope checks, file identity/hash/mode/size capture,
content/revision/data-epoch preconditions, descriptor-relative patch transactions, rollback
records and `result_unknown` evidence.  The read-only checkpoint port never restores files;
restore is brokered and approval-bound.

## Capture contract

`WorkspaceFileEdit` pins the captured identity of each path: `path`, the bounded text `contents`,
the executable `mode` bit, the observed `size_bytes` and the content `sha256` digest.  The digest
and size are computed from the exact bytes that were read (fingerprint re-checked before/after) and
default to `0`/empty for a path that was missing at capture time, so an old checkpoint stays
deserializable while a fresh capture is verifiable byte-for-byte.

## Evidence gate

- `er_restore_symlink_or_out_of_scope_path_is_denied` checks checkpoint schema/path scope, no-follow
  filesystem capture and the non-restoring port boundary.
- `er_workspace_revision_changed_during_restore_is_denied` checks expected revision/data epoch,
  prepare/finish restore evidence, approval/context invalidation and patch transaction fencing.
- `er_rollback_failure_is_unknown` checks pending transaction reconciliation and explicit Unknown
  outcomes for rollback/restore failures.
- `er18_daemon_checkpoint_fixtures_cover_card_exit_conditions` pins the daemon-side capture binding
  (`journal_sha256(&bytes)`, `before.len()`) and every ER-18 card scenario to a behavioural fixture.
- Daemon lib fixtures (`cargo test -p kiana-daemon --lib checkpoint`):
  `checkpoint_capture_records_hash_size_and_mode`, `checkpoint_capture_records_executable_mode`,
  `checkpoint_capture_denies_out_of_scope_and_protected_paths`, `checkpoint_capture_denies_symlink_targets`,
  `checkpoint_preview_reports_changes_without_writing`,
  `checkpoint_restore_applies_target_and_verifies_revision`,
  `checkpoint_restore_revision_conflict_does_not_write` (external writer stops restore),
  `checkpoint_restore_deletes_missing_and_adds_new_files` (empty/new file + rename-equivalent),
  `checkpoint_restore_rollback_keeps_prior_content_after_failure` (bounded commit failure rolls back).

The GitHub workflow runs the P2-K4 checkpoint fixture, the CP-18 source guard, this source guard and
the daemon lib checkpoint fixtures plus target-scoped compilation.  Local runtime tests are not run.

## Limits

This is source plus CI-fixture evidence only, at proof level `source`.  It does not claim physical
power-loss/crash-at-fsync atomicity, disk-full behaviour beyond the bounded io-failure fixture,
cross-host filesystem locking, external artifact-store durability or ER-19 process fencing proof.
