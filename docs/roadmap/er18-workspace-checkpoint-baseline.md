# ER-18 workspace checkpoint transaction baseline

## Scope

ER-18 binds workspace checkpoints to immutable file snapshots and the same ControlPlane restore
action: path/symlink/hardlink scope checks, file mode/content/revision/data-epoch preconditions,
descriptor-relative patch transactions, rollback records and `result_unknown` evidence.  The
read-only checkpoint port never restores files; restore is brokered and approval-bound.

## Evidence gate

- `er_restore_symlink_or_out_of_scope_path_is_denied` checks checkpoint schema/path scope, no-follow
  filesystem capture and the non-restoring port boundary.
- `er_workspace_revision_changed_during_restore_is_denied` checks expected revision/data epoch,
  prepare/finish restore evidence, approval/context invalidation and patch transaction fencing.
- `er_rollback_failure_is_unknown` checks pending transaction reconciliation and explicit Unknown
  outcomes for rollback/restore failures.

The GitHub workflow runs the existing P2-K4 checkpoint fixture and CP-18 source guard, then this
guard and workspace test-target compilation.  Local runtime tests are not run.

## Limits

This is source plus CI-fixture evidence only.  It does not claim physical power-loss atomicity,
cross-host filesystem locking, external artifact-store durability or ER-19 process fencing proof.
