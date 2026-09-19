//! CAP-27 source guard for long-running process handles and PTY continuation.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-27 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn process_continuations_are_opaque_owner_scoped_and_supervised() {
    let job = include_str!("../../kiana-domain/src/job_handle.rs");
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let leases = include_str!("../../kiana-domain/src/resource_leases.rs");
    let sessions = include_str!("../src/sessions.rs");
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let receipts = include_str!("../src/receipts.rs");
    let er19 = include_str!("er19_process_handle_guard.rs");
    let h17 = include_str!("../../kiana-domain/tests/h17_job_handle.rs");
    let baseline = include_str!("../../docs/roadmap/cap27-process-handle-pty-baseline.md");

    require(
        job,
        &[
            "JobHandle",
            "start_request_id",
            "start_invocation_id",
            "owner_id",
            "session_id",
            "project_digest",
            "authority_epoch",
            "process_group_id",
            "expires_at_unix_ms",
            "handle_digest",
            "job_handle_digest_mismatch",
        ],
        "opaque handle",
    );
    require(
        execution,
        &[
            "process.start",
            "process.poll",
            "process.stdin",
            "process.resize",
            "process.stop",
            "JobHandle::new",
            "validate_job_handle",
            "check_owner",
            "job_handle_expired",
            "job_handle_owner_mismatch",
            "job_handle_scope_mismatch",
            "process_handle_unavailable_after_restart",
            "query_extends_lease",
        ],
        "continuation operations",
    );
    require(
        supervisor,
        &[
            "ProcessSupervisor::stop",
            "leader_reaped",
            "observe_process_group",
            "StopReport",
        ],
        "supervised stop",
    );
    require(
        leases,
        &[
            "ResourceLease",
            "validate_current",
            "fence_token",
            "resource_lease_expired",
        ],
        "resource fence",
    );
    require(
        sessions,
        &[
            "durable_path_locks",
            "LOCK_NB",
            "path_lock_conflict",
            "owner",
        ],
        "workspace lease",
    );
    require(
        actions,
        &[
            "process.start",
            "process.poll",
            "process.stdin",
            "process.resize",
            "process.stop",
        ],
        "action catalog",
    );
    require(
        ports,
        &["wait_for_cancellation", "stop_confirmed"],
        "port stop fence",
    );
    require(
        receipts,
        &[
            "effect_known",
            "stop_confirmed",
            "fenced",
            "CapabilityExecutionState::Unknown",
        ],
        "receipt",
    );
    require(
        er19,
        &[
            "JobHandle",
            "process_handle_unavailable_after_restart",
            "ProcessSupervisor::stop",
        ],
        "ER-19 regression",
    );
    require(
        h17,
        &["job_handle_round_trip", "job_handle_digest_mismatch"],
        "H17 regression",
    );
    require(
        baseline,
        &[
            "foreign_or_expired_process_handle_is_rejected",
            "stdin_cannot_reuse_a_revoked_execution_scope",
            "pty_disconnect_does_not_orphan_processes",
        ],
        "CAP-27 acceptance card",
    );
    assert!(execution.contains("query_extends_lease\":false"));
    assert!(!execution.contains("query_extends_lease\":true"));
    assert!(!execution.contains("CapabilityBroker::new"));
}

#[test]
fn pty_is_explicit_and_disconnect_or_stop_cannot_claim_success() {
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let capabilities = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let domain = include_str!("../../kiana-domain/src/capabilities.rs");
    let baseline = include_str!("../../docs/roadmap/cap27-process-handle-pty-baseline.md");

    require(
        execution,
        &[
            "let tty = arguments[\"pty\"] == true",
            "open_terminal",
            "pty_unavailable",
            "pty_backend_unsupported",
            "pty_rows_invalid",
            "pty_cols_invalid",
            "process_has_no_pty",
            "pty_resize_failed",
            "capture_complete",
            "stop_confirmed",
        ],
        "PTY boundary",
    );
    require(
        capabilities,
        &[
            "ProcessSupervisor::stop",
            "stop_report",
            "shell_result_unknown:cancel_stop_unconfirmed",
        ],
        "shell stop",
    );
    require(
        domain,
        &[
            "CapabilityStopState",
            "CapabilityEffectState",
            "Unknown",
            "fenced",
        ],
        "effect state",
    );
    require(
        baseline,
        &[
            "stdout/stderr 合流",
            "控制字符",
            "OSC",
            "result_unknown",
            "automatic_retry:false",
        ],
        "PTY limits",
    );
}
