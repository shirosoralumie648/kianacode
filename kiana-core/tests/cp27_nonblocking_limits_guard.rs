//! CP-27 source guard for non-blocking storage, trusted clocks and bounded load behavior.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CP-27 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cp_slow_storage_does_not_starve_cancellation() {
    let eventlog = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let process = include_str!("../../kiana-daemon/src/execution_control.rs");
    let mcp = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let workspace = include_str!("../../kiana-daemon/src/workspace_checkpoints.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let stream = include_str!("../../kiana-daemon/src/run_stream.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    require(
        eventlog,
        &[
            "bounded blocking I/O",
            "MAX_STORAGE_WORKERS",
            "try_acquire_owned",
            "eventlog_worker_queue_full",
            "spawn_blocking",
            "with_store_state",
        ],
        "EventLog worker boundary",
    );
    require(
        daemon,
        &[
            "spawn_blocking",
            "tokio::time::timeout",
            "wait_for_cancellation",
            "output_drain_timeout",
            "stop_confirmed",
        ],
        "Capability I/O boundary",
    );
    require(
        process,
        &[
            "Semaphore",
            "process_capacity_exceeded",
            "process_deadline_invalid",
            "spawn_blocking",
            "wait_for_cancellation",
            "tokio::time::timeout",
        ],
        "process boundary",
    );
    require(
        mcp,
        &[
            "spawn_blocking",
            "timeout_at",
            "mcp_write_timeout",
            "mcp_response_timeout",
        ],
        "MCP boundary",
    );
    require(
        workspace,
        &[
            "spawn_blocking",
            "capture_checkpoint_files",
            "checkpoint_capture_join_failed",
        ],
        "workspace boundary",
    );
    require(
        memory,
        &[
            "spawn_blocking",
            "Handle::block_on",
            "read_records",
            "write_record_scoped",
        ],
        "memory boundary",
    );
    require(
        stream,
        &[
            "tokio::time::timeout",
            "stream_subscription_lagged",
            "terminal",
        ],
        "stream cancellation boundary",
    );
    require(
        ports,
        &[
            "wait_for_cancellation",
            "ObservabilityQueue",
            "capacity",
            "ClockPort",
        ],
        "ports boundary",
    );

    for source in [eventlog, daemon, process, mcp, workspace, memory] {
        assert!(
            !source.contains("std::thread::sleep") && !source.contains("block_on(async")
                || source.contains("spawn_blocking"),
            "blocking I/O must be isolated from async request workers"
        );
    }
}

#[test]
fn cp_clock_rollback_cannot_extend_authority() {
    let clock = include_str!("../../kiana-domain/src/clock.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let fence = include_str!("../src/security_fence.rs");
    let approvals = include_str!("../src/approvals.rs");
    let journal_approvals = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let business = include_str!("../src/company_business.rs");
    let cell = include_str!("../src/cell_registry.rs");
    let authority = include_str!("../src/authority.rs");

    require(
        clock,
        &[
            "ClockObservation",
            "ClockTrust::Rollback",
            "validate_transition_from",
            "clock_transition_rollback_untrusted",
            "clock_untrusted",
            "allows_before",
            "clamp_deadline",
        ],
        "clock observation",
    );
    require(
        ports,
        &[
            "pub trait ClockPort",
            "require_trusted_deadline",
            "monotonic_now_ms",
            "clock_deadline_expired",
            "ClockObservation::observe",
        ],
        "clock port",
    );
    require(
        fence,
        &[
            "validate_authority_fence",
            "validate_current",
            "authority_epoch",
            "policy_revision",
            "FACT_FENCE_MISMATCH",
        ],
        "authority fence",
    );
    require(
        approvals,
        &[
            "watch_cancel",
            "cancelled:before_approval",
            "approval_expired",
        ],
        "approval expiry",
    );
    require(
        journal_approvals,
        &[
            "approval_clock_rollback",
            "approval_live_payload_limit",
            "expires_at",
        ],
        "durable approval expiry",
    );
    require(
        business,
        &["business_clock_rollback", "now_unix_ms", "deadline"],
        "company deadline",
    );
    require(
        cell,
        &[
            "deadline_unix_ms",
            "spawn_deadline_expired",
            "expires_at_unix_ms",
        ],
        "cell deadline",
    );
    require(
        authority,
        &["authority_epoch", "authority_revision", "monotonic"],
        "authority version",
    );
}

#[test]
fn cp_pending_and_event_limits_fail_without_resource_leak() {
    let eventlog = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let events = include_str!("../src/events.rs");
    let approvals = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let capabilities = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let process = include_str!("../../kiana-daemon/src/execution_control.rs");
    let queue = include_str!("../../kiana-ports/src/observability_queue.rs");
    let model_budget = include_str!("../src/model_budget.rs");
    let runner_budget = include_str!("../../kiana-runner/src/budget.rs");
    let health = include_str!("../src/health.rs");

    require(
        eventlog,
        &[
            "MAX_JOURNAL_LOG_BYTES",
            "eventlog_disk_limit",
            "eventlog_event_size_limit",
            "eventlog_frame_size_limit",
            "read_from",
            "check_disk_capacity",
            "eventlog_worker_queue_full",
        ],
        "EventLog limits",
    );
    require(
        journal,
        &[
            "journal_batch_event_limit",
            "journal_event_size_limit",
            "journal_payload_depth_limit",
        ],
        "journal limits",
    );
    require(
        events,
        &[
            "event_payload_depth_limit",
            "event_payload_size_limit",
            "MAX_JOURNAL_EVENT_BYTES",
        ],
        "event payload limits",
    );
    require(
        approvals,
        &["approval_live_payload_limit", "approval_clock_rollback"],
        "approval limits",
    );
    require(
        patch,
        &[
            "workspace_journal_pending_limit",
            "workspace_rollback_revision_conflict",
            "result_unknown",
        ],
        "patch limits",
    );
    require(
        capabilities,
        &[
            "EXEC_OUTPUT_MAX_BYTES",
            "output_total_limit",
            "output_drain_timeout",
            "READ_CHUNK_SIZE",
        ],
        "tool output limits",
    );
    require(
        process,
        &[
            "process_capacity_exceeded",
            "process_deadline_invalid",
            "Semaphore",
            "stop_confirmed",
        ],
        "process limits",
    );
    require(
        queue,
        &[
            "critical_queue_full",
            "best_effort_queue_full",
            "capacity",
            "ObservabilityQueue",
        ],
        "telemetry queue limits",
    );
    require(
        model_budget,
        &[
            "reserve_prepared",
            "model_deadline_expired",
            "usage_known",
            "unknown_attempts",
        ],
        "model budget limits",
    );
    require(
        runner_budget,
        &[
            "max_model_steps_per_turn",
            "max_attempts_per_task",
            "max_wall_time_per_task",
        ],
        "Runner limits",
    );
    require(
        health,
        &[
            "eventlog_capability_or_cursor_limit",
            "capacity",
            "degraded",
        ],
        "health limits",
    );
}
