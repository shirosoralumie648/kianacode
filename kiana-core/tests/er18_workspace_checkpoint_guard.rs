//! ER-18 source guard for workspace checkpoint transactions and restore evidence.

#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::{compact_code, item_source};

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
            "WorkspaceFileEdit",
            "project_root",
            "path_allow",
            "workspace_revision",
            "data_epoch",
            "size_bytes",
            "sha256",
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
    let adapter = compact_code(&item_source(daemon, "async fn execute("));
    assert!(adapter.contains("expected_revision"));
    assert!(adapter.contains("restore_checkpoint(&checkpoint,&revision)"));
    for marker in [
        "checkpoint_revision_conflict",
        "checkpoint_data_epoch_changed",
        "workspace.restored",
    ] {
        assert!(
            core.contains(marker),
            "ER-18 restore evidence marker missing in core: {marker}"
        );
    }
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

#[test]
fn er18_daemon_checkpoint_fixtures_cover_card_exit_conditions() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let domain = include_str!("../../kiana-domain/src/platform.rs");

    // The capture contract must pin identity/hash/mode/size, not just text.
    require(
        domain,
        &["WorkspaceFileEdit", "size_bytes", "sha256", "executable"],
        "capture contract",
    );
    require(
        patch,
        &[
            "kiana_domain::journal_sha256(&bytes)",
            "let size_bytes = before.len()",
            "checkpoint_relative_path",
            "checkpoint_protected_path",
            "checkpoint_file_mode_changed",
        ],
        "capture binding",
    );
    // Success/regression scenarios from the ER-18 card must exist as behavioural fixtures.
    require(
        patch,
        &[
            "checkpoint_capture_records_hash_size_and_mode",
            "checkpoint_capture_records_executable_mode",
            "checkpoint_capture_denies_out_of_scope_and_protected_paths",
            "checkpoint_capture_denies_symlink_targets",
            "checkpoint_preview_reports_changes_without_writing",
            "checkpoint_restore_applies_target_and_verifies_revision",
            "checkpoint_restore_revision_conflict_does_not_write",
            "checkpoint_restore_deletes_missing_and_adds_new_files",
            "checkpoint_restore_rollback_keeps_prior_content_after_failure",
        ],
        "daemon checkpoint fixtures",
    );
}
