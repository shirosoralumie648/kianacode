//! PD-13 source guard for pending invocation, runner continuation and workspace checkpoint.

#[test]
fn recovery_material_requires_explicit_snapshot_binding_and_checkpoint_boundaries() {
    let recovery = include_str!("../src/recovery.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let checkpoints = include_str!("../src/workspace_checkpoints.rs");
    let domain = include_str!("../../kiana-domain/src/capabilities.rs");
    let resume = include_str!("../../kiana-domain/src/invocation_resume.rs");

    for marker in [
        "rebuild_pending_invocation",
        "run_snapshot_pending_invocation_missing",
        "run_snapshot_stale",
        "resume_binding",
        "pending_invocation",
    ] {
        assert!(
            recovery.contains(marker)
                || lifecycle.contains(marker)
                || checkpoints.contains(marker)
                || domain.contains(marker)
                || resume.contains(marker),
            "PD-13 recovery marker missing: {marker}"
        );
    }
    for marker in [
        "old_approvals_invalidated",
        "runner_context_invalidated",
        "checkpoint_restore_scope_changed",
        "checkpoint_data_epoch_changed",
    ] {
        assert!(
            checkpoints.contains(marker) || recovery.contains(marker),
            "PD-13 re-admission marker missing: {marker}"
        );
    }
    for forbidden in [
        "transcript_auto_resume",
        "ui_auto_resume",
        "CapabilityBrokerPort::execute",
    ] {
        assert!(!recovery.contains(forbidden));
    }
}
