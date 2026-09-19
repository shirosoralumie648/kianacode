//! CAP-23 source guard for fair admission, conservative footprints and cancellation fencing.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-23 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn admission_is_bounded_fair_and_never_an_execution_loop() {
    let scheduler = include_str!("../src/capability_scheduler.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let cell = include_str!("../src/cell_registry.rs");
    let sessions = include_str!("../src/sessions.rs");
    let daemon = include_str!("../../kiana-daemon/src/execution_control.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");

    require(
        scheduler,
        &[
            "CapabilityAdmissionScheduler",
            "MAX_ACTIVE_ADMISSIONS",
            "MAX_PENDING_ADMISSIONS",
            "FootprintMode::Read",
            "FootprintMode::Write",
            "FootprintMode::Unknown",
            "canonical_paths",
            "earlier_conflict",
            "active_conflict",
            "quarantine_conflict",
            "cancellation.changed()",
            "cancelled:before_dispatch",
            "capability_scheduler_queue_full",
            "AdmissionOutcome::Unknown",
            "state.quarantined",
            "never invokes a handler",
        ],
        "admission scheduler",
    );
    require(
        capabilities,
        &[
            "admission_scheduler",
            ".acquire(request",
            "begin_cell_capability_from_request",
            "finish_cell_capability",
            "AdmissionOutcome::Unknown",
        ],
        "ControlPlane dispatch gate",
    );
    require(
        cell,
        &[
            "path_lock_conflict",
            "budget_ledger",
            ".reserve(plan.budget_reservation)",
            "begin_capability",
            "finish_capability",
            "cell_capability_concurrency_exceeded",
            "resources_released",
        ],
        "CellRegistry authority",
    );
    require(
        sessions,
        &[
            "acquire_durable_path_locks",
            "LOCK_NB",
            "path_lock_conflict",
        ],
        "cross-process lock",
    );
    require(
        daemon,
        &[
            "acquire_workspace_resources",
            "capacity",
            "ProcessSupervisor",
        ],
        "daemon resource boundary",
    );
    require(
        runner,
        &[
            "pending_tools.drain(..)",
            "cancelled:queued",
            "ToolCancelled",
        ],
        "queued cancellation",
    );
    require(
        baseline,
        &[
            "overlapping_write_scopes_never_execute_concurrently",
            "parallel_calls_cannot_overspend_parent_budget",
            "cancelled_queued_call_never_spawns",
        ],
        "CAP-23 acceptance card",
    );
    assert!(!scheduler.contains("CapabilityBroker"));
    assert!(!scheduler.contains("tokio::spawn"));
    assert!(!scheduler.contains("model loop"));
}

#[test]
fn unknown_and_cancelled_admissions_have_distinct_boundaries() {
    let scheduler = include_str!("../src/capability_scheduler.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let cancellation = include_str!("../../kiana-domain/src/cancellation.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");

    assert!(scheduler.contains("quarantined.push(footprint)"));
    assert!(scheduler.contains("AdmissionOutcome::Unknown"));
    assert!(capabilities.contains("result_unknown:cancel_stop_unconfirmed"));
    assert!(capabilities.contains("not_executed"));
    assert!(cancellation.contains("RunCancellationState::ResultUnknown"));
    assert!(lifecycle.contains("run.result_unknown"));
}
