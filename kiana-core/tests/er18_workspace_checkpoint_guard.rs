//! ER-18 source guard for workspace checkpoint transactions and restore evidence.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-18 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn er_restore_symlink_or_out_of_scope_path_is_denied() {
    let core = include_str!("../src/workspace_checkpoints.rs");
    let daemon = include_str!("../../kiana-daemon/src/workspace_checkpoints.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    require(
        domain,
        &[
            "WorkspaceCheckpoint",
            "WorkspaceFileSnapshot",
            "project_root",
            "path_allow",
            "workspace_revision",
            "data_epoch",
            "files",
        ],
        "checkpoint contract",
    );
    require(
        core,
        &[
            "checkpoint_path_allowed",
            "checkpoint_protected_path",
            "checkpoint_path_denied",
            "checkpoint_restore_scope_changed",
            "checkpoint_source_epoch",
            "checkpoint_data_epoch_changed",
            "checkpoint_snapshot_not_authoritative",
        ],
        "ControlPlane path/scope guard",
    );
    require(
        daemon,
        &[
            "capture_checkpoint_files",
            "preview_checkpoint",
            "restore_checkpoint",
            "checkpoint_source_revoked",
            "checkpoint_path_denied",
        ],
        "daemon filesystem guard",
    );
    require(
        patch,
        &[
            "reject_symlink_components",
            "descriptor-relative",
            "checkpoint_hardlink_denied",
            "O_NOFOLLOW",
            "workspace_publish_revision_conflict",
            "checkpoint_file_mode_changed",
            "workspace_revision",
        ],
        "patch path guard",
    );
    require(
        ports,
        &[
            "WorkspaceCheckpointPort",
            "capture_files",
            "preview",
            "Restoring files is deliberately absent",
        ],
        "checkpoint port",
    );
}

#[test]
fn er_workspace_revision_changed_during_restore_is_denied() {
    let core = include_str!("../src/workspace_checkpoints.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let daemon = include_str!("../../kiana-daemon/src/workspace_checkpoints.rs");
    let fixture = include_str!("p2_k4_01_checkpoint.rs");

    require(
        core,
        &[
            "expected_revision",
            "checkpoint_revision_conflict",
            "prepare_checkpoint_restore",
            "finish_checkpoint_restore",
            "workspace.checkpoint.restore",
            "old_approvals_invalidated",
            "runner_context_invalidated",
        ],
        "restore transaction gate",
    );
    require(
        patch,
        &[
            "current_revision",
            "target_revision",
            "rollback_patch_guarded",
            "workspace_rollback_revision_conflict",
            "workspace_transaction",
            "patch-transaction.v1",
        ],
        "patch transaction",
    );
    require(
        daemon,
        &[
            "checkpoint_revision_conflict",
            "checkpoint_data_epoch_changed",
            "workspace.restored",
        ],
        "restore evidence",
    );
    require(
        fixture,
        &[
            "checkpoint_restore_invalidates_approvals_and_runner_context",
            "checkpoint_revision_conflict",
            "checkpoint_data_epoch_changed",
        ],
        "checkpoint fixture",
    );
}

#[test]
fn er_rollback_failure_is_unknown() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let core = include_str!("../src/workspace_checkpoints.rs");
    let daemon = include_str!("../../kiana-daemon/src/workspace_checkpoints.rs");

    require(
        patch,
        &[
            "rollback_patch_guarded",
            "workspace_rollback_revision_conflict",
            "result_unknown:workspace_transaction_reconciliation_required",
            "result_unknown:apply_patch_rollback_failed",
            "pending_patch_transactions",
            "workspace_journal_finish_failed",
        ],
        "rollback transaction",
    );
    require(
        core,
        &[
            "result_unknown:checkpoint_restore_result_invalid",
            "finish_checkpoint_restore",
        ],
        "ControlPlane restore result",
    );
    require(
        daemon,
        &[
            "result_unknown:checkpoint_restore_join_failed",
            "restore_checkpoint",
        ],
        "daemon restore result",
    );
    for source in [patch, core, daemon] {
        assert!(!source.contains("rollback_failure_is_success"));
        assert!(!source.contains("ignore_restore_error"));
    }
}
