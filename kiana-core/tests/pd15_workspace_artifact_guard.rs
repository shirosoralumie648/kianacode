//! PD-15 source guard for workspace patch/checkpoint/diff/undo and artifact references.

#[test]
fn workspace_restore_requires_checkpoint_and_artifact_scope_fences() {
    let core = include_str!("../src/workspace_checkpoints.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let artifacts = include_str!("../src/artifacts.rs");
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    for marker in [
        "checkpoint_revision_conflict",
        "checkpoint_data_epoch_changed",
        "checkpoint_restore_scope_changed",
        "old_approvals_invalidated",
        "runner_context_invalidated",
        "workspace_revision",
        "ArtifactRef",
        "validate_artifact_reference_content",
    ] {
        assert!(
            core.contains(marker)
                || patch.contains(marker)
                || artifacts.contains(marker)
                || domain.contains(marker),
            "PD-15 marker missing: {marker}"
        );
    }
    for forbidden in [
        "restore_from_transcript",
        "undo_without_checkpoint",
        "CapabilityBrokerPort::execute",
    ] {
        assert!(!core.contains(forbidden) && !patch.contains(forbidden));
    }
}
