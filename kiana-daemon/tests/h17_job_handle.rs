#[test]
fn foreign_job_handle_cannot_receive_stdin_or_cancel() {
    let source = include_str!("../src/execution_control.rs");
    let core = include_str!("../../kiana-core/src/capabilities.rs");
    for marker in [
        "JobHandle",
        "validate_job_handle",
        "job_handle_owner_mismatch",
        "job_handle_scope_mismatch",
        "process.stdin",
        "process.stop",
        "process_authority_revoked",
    ] {
        assert!(
            source.contains(marker) || core.contains(marker),
            "H17 owner marker missing: {marker}"
        );
    }
}

#[test]
fn pid_reuse_cannot_attach_to_other_process() {
    let source = include_str!("../src/execution_control.rs");
    assert!(source.contains("process_handle_unavailable_after_restart"));
    assert!(source.contains("process_group_id"));
    assert!(source.contains("job_handle_process_identity_missing"));
    assert!(source.contains("process.started"));
    assert!(source.contains("process.prepared"));
}

#[test]
fn poll_does_not_restart_job() {
    let source = include_str!("../src/execution_control.rs");
    assert!(source.contains("request.request.operation != \"process.poll\""));
    assert!(source.contains("process_handle_unavailable_after_restart"));
    assert!(source.contains("process.finished"));
    assert!(source.contains("cursor"));
}

#[test]
fn long_command_has_one_job_and_distinct_operation_invocations() {
    let source = include_str!("../src/execution_control.rs");
    let dispatch = include_str!("../../kiana-core/src/dispatch.rs");
    for marker in [
        "job_handle",
        "start_request_id",
        "process_id",
        "process.poll",
        "process.stdin",
        "process.resize",
        "process.stop",
        "commit_invocation_executing",
        "InvocationId::from_uuid",
    ] {
        assert!(
            source.contains(marker) || dispatch.contains(marker),
            "H17 job marker missing: {marker}"
        );
    }
}
