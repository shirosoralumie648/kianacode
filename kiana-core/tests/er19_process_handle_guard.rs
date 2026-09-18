//! ER-19 source guard for process/job handles, fencing and stop evidence.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-19 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn er_stale_handle_cannot_stop_new_execution() {
    let job = include_str!("../../kiana-domain/src/job_handle.rs");
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let sessions = include_str!("../src/sessions.rs");
    let leases = include_str!("../../kiana-domain/src/resource_leases.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    require(
        job,
        &[
            "JobHandle",
            "job_id",
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
        "JobHandle identity",
    );
    require(
        execution,
        &[
            "JobHandle::new",
            "validate_job_handle",
            "check_owner",
            "job_handle_identity_mismatch",
            "job_handle_expired",
            "job_handle_owner_mismatch",
            "job_handle_scope_mismatch",
            "process_handle_unavailable_after_restart",
            "process_group_id",
        ],
        "process continuation",
    );
    require(
        sessions,
        &[
            "canonical_resource_set",
            "LOCK_NB",
            "path_lock_conflict",
            "owner",
        ],
        "path lease",
    );
    require(
        leases,
        &[
            "ResourceLease",
            "fence_token",
            "successor",
            "authority_epoch",
            "expires_at_unix_ms",
            "resource_lease_fence_token_reused",
            "validate_current",
        ],
        "resource fencing",
    );
    require(
        ports,
        &["fencing_token", "wait_for_cancellation", "stop_confirmed"],
        "port stop fence",
    );
}

#[test]
fn er_child_escape_or_leader_exit_is_unknown() {
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let capabilities = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let domain = include_str!("../../kiana-domain/src/capabilities.rs");
    let receipts = include_str!("../src/receipts.rs");
    let fixture = include_str!("../../kiana-daemon/tests/h15_output_limits.rs");

    require(
        execution,
        &[
            "process_group_id",
            "process_group_exists",
            "leader_reaped",
            "terminate_process_group",
            "stop_confirmed",
            "result_unknown",
            "capture_complete",
            "automatic_retry",
        ],
        "supervisor stop evidence",
    );
    require(
        capabilities,
        &[
            "process_group_exists",
            "leader_reaped",
            "stop_confirmed",
            "shell_result_unknown:cancel_stop_unconfirmed",
            "output_drain_timeout",
        ],
        "shell process boundary",
    );
    require(
        domain,
        &[
            "CapabilityStopState",
            "CapabilityEffectState",
            "Unknown",
            "stop_confirmed",
            "fenced",
        ],
        "effect dimensions",
    );
    require(
        receipts,
        &[
            "effect_known",
            "stop_confirmed",
            "fenced",
            "CapabilityExecutionState::Unknown",
        ],
        "Receipt stop projection",
    );
    require(
        fixture,
        &["output_total_limit", "expired_output_reference"],
        "output/process fixture",
    );
}

#[test]
fn er_lease_expiry_does_not_release_live_process() {
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let job = include_str!("../../kiana-domain/src/job_handle.rs");
    let leases = include_str!("../../kiana-domain/src/resource_leases.rs");
    let sessions = include_str!("../src/sessions.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");

    require(
        execution,
        &[
            "expires_at_unix_ms",
            "job_handle_expired",
            "query_extends_lease",
            "authority_revoked",
            "process.finished",
            "std::mem::forget(_leases)",
        ],
        "live process lease",
    );
    require(
        job,
        &[
            "expires_at_unix_ms",
            "job_handle_expired",
            "authority_epoch",
            "handle_digest",
        ],
        "job expiry",
    );
    require(
        leases,
        &[
            "expires_at_unix_ms",
            "resource_lease_expired",
            "resource_lease_authority_epoch_stale",
            "fence_token",
        ],
        "resource expiry",
    );
    require(
        sessions,
        &[
            "durable_path_locks",
            "LOCK_NB",
            "release_builder_path_locks",
            "owner",
        ],
        "session lease",
    );
    require(
        lifecycle,
        &[
            "await_capability_stop",
            "stop_confirmed",
            "ResultUnknown",
            "run.cancelled",
        ],
        "lifecycle stop",
    );
}
