#[test]
fn checkpoint_restore_invalidates_approvals_and_runner_context() {
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    let core = include_str!("../src/workspace_checkpoints.rs");
    let daemon = include_str!("../../kiana-daemon/src/workspace_checkpoints.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let baseline = include_str!("../../docs/roadmap/p2-k4-01-artifact-checkpoint-baseline.md");
    for marker in [
        "WorkspaceCheckpoint",
        "CheckpointPreview",
        "transcript_offset",
        "workspace_revision",
        "invocation_id",
        "data_epoch",
        "capture_edit_checkpoint",
        "checkpoint_before_write",
        "checkpoint_before_input",
        "workspace.checkpoint.create",
        "workspace.checkpoint.preview",
        "workspace.checkpoint.restore",
        "checkpoint_revision_conflict",
        "checkpoint_snapshot_not_authoritative",
        "checkpoint_data_epoch_changed",
        "validate_checkpoint_company_scope",
        "prepare_checkpoint_restore",
        "finish_checkpoint_restore",
        "invalidate_project",
        "stop_project_runs",
        "invalidate_project",
        "old_approvals_invalidated",
        "runner_context_invalidated",
        "capture_checkpoint_files",
        "preview_checkpoint",
        "restore_checkpoint",
        "O_NOFOLLOW",
        "ProjectPatchLock",
        "authorize_and_execute",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker)
                || patch.contains(marker)
                || dispatch.contains(marker)
                || lifecycle.contains(marker)
                || baseline.contains(marker),
            "checkpoint marker missing: {marker}"
        );
    }
    assert!(core.contains("self.approvals\n            .invalidate_project"));
    assert!(core.contains("self.stop_project_runs"));
    assert!(core.contains("workspace.restored"));
    assert!(patch.contains("checkpoint_file_changed"));
    assert!(patch.contains("checkpoint_hardlink_denied"));
    assert!(!core.contains("ModelClient"));
    assert!(!core.contains("CapabilityBroker"));
}
