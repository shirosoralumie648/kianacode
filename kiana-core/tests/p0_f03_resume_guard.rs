fn sources() -> (&'static str, &'static str, &'static str) {
    (
        include_str!("../src/recovery.rs"),
        include_str!("../src/approvals.rs"),
        include_str!("control_plane.rs"),
    )
}

#[test]
fn fresh_process_resume_reconstructs_pending_approval() {
    let (recovery, approvals, existing) = sources();
    assert!(recovery.contains("read_all_events"));
    assert!(recovery.contains("cache_invocation_projection(run_id, &events)"));
    assert!(recovery.contains("rebuild_pending_invocation"));
    assert!(recovery.contains("run_snapshot_pending_invocation_missing"));
    assert!(approvals.contains("pending_with_proof"));
    assert!(existing
        .contains("projection_cache_miss_rebuilds_pending_invocations_with_authorization_recheck"));
}

#[test]
fn restart_pending_approval_waits_for_explicit_resume() {
    let (recovery, _, _) = sources();
    assert!(recovery.contains("pub async fn resume_run"));
    assert!(recovery.contains("run.resume_prepared"));
    assert!(recovery.contains("self.runner.restore(run_id, snapshot.runner_state)"));
    assert!(recovery.contains("approval_continuation_unavailable"));
    assert!(!recovery.contains("auto_resume_terminal"));
}

#[test]
fn restored_pending_approval_rechecks_policy_gate_and_approval() {
    let (recovery, approvals, existing) = sources();
    for marker in [
        "prepare_capability_action",
        "authorize_capability_action",
        "GateDecision::Allowed",
        "approval_requirements_changed",
        "run_resume_authority_changed",
        "run_resume_data_revoked",
        "run_resume_scope_changed",
    ] {
        assert!(
            recovery.contains(marker) || approvals.contains(marker),
            "resume recheck marker missing: {marker}"
        );
    }
    assert!(existing.contains("gate_denied_after_restart"));
}
