#[test]
fn cp18_run_snapshot_and_checkpoint_restore_are_digest_scoped_and_non_executing() {
    let domain = include_str!("../../kiana-domain/src/capabilities.rs");
    let resume = include_str!("../../kiana-domain/src/invocation_resume.rs");
    let recovery = include_str!("../src/recovery.rs");
    let workspace = include_str!("../src/workspace_checkpoints.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let daemon = include_str!("../../kiana-daemon/src/workspace_checkpoints.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let checkpoint_fixture = include_str!("p2_k4_01_checkpoint.rs");
    let resume_fixture = include_str!("p0_g03_resume_guard.rs");

    for marker in [
        "pub struct RunSnapshot",
        "runner_state_digest",
        "pending_invocation",
        "authority_revision",
        "data_epoch",
        "resumable",
        "kiana.run-snapshot.v1",
    ] {
        assert!(
            domain.contains(marker),
            "CP-18 snapshot marker missing: {marker}"
        );
    }
    for marker in [
        "InvocationResumeBinding",
        "pending_batch_digest",
        "validate_against",
        "invocation_resume_binding_changed",
        "catalog_digest",
        "sandbox_digest",
    ] {
        assert!(
            resume.contains(marker),
            "CP-18 resume binding marker missing: {marker}"
        );
    }
    for marker in [
        "checkpoint_run",
        "run.snapshot",
        "runner_state_digest",
        "run_snapshot_invalid",
        "run_resume_authority_changed",
        "run_resume_data_revoked",
        "run_resume_scope_changed",
        "run_snapshot_stale",
        "append_expected(claim, Some(last_version))",
        "self.runner.restore(run_id, snapshot.runner_state)",
        ".drive_run(",
    ] {
        assert!(
            recovery.contains(marker),
            "CP-18 recovery marker missing: {marker}"
        );
    }
    for marker in [
        "capture_edit_checkpoint",
        "checkpoint_before_write",
        "checkpoint_before_input",
        "workspace.checkpoint.create",
        "workspace.checkpoint.preview",
        "workspace.checkpoint.restore",
        "checkpoint_revision_conflict",
        "checkpoint_data_epoch_changed",
        "old_approvals_invalidated",
        "runner_context_invalidated",
        "validate_checkpoint_company_scope",
    ] {
        assert!(
            workspace.contains(marker),
            "CP-18 workspace marker missing: {marker}"
        );
    }
    for marker in [
        "prepare_checkpoint_restore",
        "finish_checkpoint_restore",
        "commit_confirmed",
        "execution.result_committed",
        "result_unknown",
    ] {
        assert!(
            dispatch.contains(marker),
            "CP-18 dispatch marker missing: {marker}"
        );
    }
    for marker in [
        "capture_checkpoint_files",
        "preview_checkpoint",
        "restore_checkpoint",
        "checkpoint_source_revoked",
        "checkpoint_hardlink_denied",
        "O_NOFOLLOW",
    ] {
        assert!(
            daemon.contains(marker) || patch.contains(marker),
            "CP-18 daemon checkpoint marker missing: {marker}"
        );
    }
    for marker in [
        "checkpoint_run",
        "restore_run",
        "runner_checkpoint_unsupported",
        "runner_restore_unsupported",
    ] {
        assert!(
            ports.contains(marker),
            "CP-18 port marker missing: {marker}"
        );
    }
    assert!(
        checkpoint_fixture.contains("checkpoint_restore_invalidates_approvals_and_runner_context")
    );
    assert!(resume_fixture.contains("resume_run_reuses_the_same_drive_run_path"));
    for source in [recovery, workspace, daemon] {
        assert!(!source.contains("CapabilityBroker"));
        assert!(!source.contains("ModelClient"));
    }
}
