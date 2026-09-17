#[test]
fn resume_run_reuses_the_same_drive_run_path() {
    let recovery = include_str!("../src/recovery.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");

    assert!(recovery.contains("pub async fn resume_run"));
    assert!(recovery.contains("run.resume_prepared"));
    assert!(recovery.contains("self.runner.restore(run_id, snapshot.runner_state)"));
    assert!(recovery.contains(".drive_run("));
    assert!(lifecycle.contains("pub(crate) async fn drive_run"));
    assert!(protocol.contains("pub struct ResumeRequest"));
    assert!(protocol.contains("RequestBody::Resume(ResumeRequest { run_id })"));
    assert!(daemon.contains("RequestBody::Resume(run) => self.core.resume_run"));
    assert!(!recovery.contains("tokio::spawn"));
}

#[test]
fn resume_requires_snapshot_and_rejects_stale_scope() {
    let recovery = include_str!("../src/recovery.rs");
    for marker in [
        "approval_continuation_unavailable",
        "run_snapshot_invalid",
        "run_resume_authority_changed",
        "run_resume_data_revoked",
        "run_resume_scope_changed",
        "run_snapshot_stale",
        "append_expected(claim, Some(last_version))",
    ] {
        assert!(
            recovery.contains(marker),
            "resume guard marker missing: {marker}"
        );
    }
}
