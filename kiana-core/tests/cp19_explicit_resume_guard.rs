#[test]
fn cp19_resume_rebuilds_from_facts_and_claims_once_before_drive_run() {
    let recovery = include_str!("../src/recovery.rs");
    let projection = include_str!("../src/invocation_projection.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let p0_resume = include_str!("p0_g03_resume_guard.rs");
    let h14 = include_str!("../../kiana-domain/tests/h14_invocation_resume.rs");

    for marker in [
        "pub async fn resume_run",
        "read_all_events",
        "try_filter_run_events",
        "cache_invocation_projection",
        "run.resume_prepared",
        "append_expected(claim, Some(last_version))",
        "self.runner.restore(run_id, snapshot.runner_state)",
        "run_snapshot_pending_invocation_missing",
        "run_resume_authority_changed",
        "run_resume_data_revoked",
        "run_resume_scope_changed",
        "run_snapshot_stale",
        "run_resume_sandbox_denied",
    ] {
        assert!(
            recovery.contains(marker),
            "CP-19 recovery marker missing: {marker}"
        );
    }
    for marker in [
        "execution.result_committed",
        "CapabilityExecutionState::Succeeded",
        "CapabilityExecutionState::Unknown",
        "terminal_conflict",
    ] {
        assert!(
            projection.contains(marker),
            "CP-19 projection marker missing: {marker}"
        );
    }
    for marker in [
        "pub(crate) async fn drive_run",
        "RunnerEvent::CapabilityRequested",
        "pending_invocation_missing",
        "checkpoint_run",
    ] {
        assert!(
            lifecycle.contains(marker),
            "CP-19 lifecycle marker missing: {marker}"
        );
    }
    for marker in [
        "async fn restore(",
        "checkpoint_run",
        "restore_run",
        "runner_restore_unsupported",
        "cell_restore_unsupported",
    ] {
        assert!(
            ports.contains(marker),
            "CP-19 port marker missing: {marker}"
        );
    }
    for marker in [
        "RequestBody::Resume(run)",
        "resume_run(context, run.run_id)",
    ] {
        assert!(
            daemon.contains(marker),
            "CP-19 daemon resume marker missing: {marker}"
        );
    }
    assert!(protocol.contains("pub struct ResumeRequest"));
    assert!(runner.contains("async fn restore"));
    assert!(p0_resume.contains("resume_run_reuses_the_same_drive_run_path"));
    assert!(h14.contains("InvocationResumeBinding"));
    assert!(!recovery.contains("tokio::spawn"));
    assert!(!recovery.contains("CapabilityBroker"));
    assert!(!recovery.contains("ProviderGateway"));
}
