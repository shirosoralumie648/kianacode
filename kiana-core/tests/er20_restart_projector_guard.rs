//! ER-20 source guard for restart projection and default-paused recovery.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-20 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn restart_never_auto_resumes_pending_run() {
    let recovery = include_str!("../src/recovery.rs");
    let projection = include_str!("../src/projection.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");

    require(
        recovery,
        &[
            "read_all_events",
            "rebuild_pending_invocation",
            "list_pending_approvals",
            "recovery_resources",
            "run_snapshot_stale",
            "pub async fn resume_run",
            "run.resume_prepared",
        ],
        "recovery rebuild",
    );
    require(
        projection,
        &[
            "invocation_state",
            "run_state",
            "read_all_events",
            "run_not_found",
            "cache_invocation_projection",
        ],
        "projection rebuild",
    );
    require(
        daemon,
        &[
            "startup_health",
            "event_store_health",
            "persisted_events",
            "HealthProbeKind::Readiness",
            "HealthSnapshot",
        ],
        "DaemonHost startup",
    );
    require(
        runner,
        &[
            "async fn checkpoint",
            "async fn restore",
            "runner_checkpoint_invalid",
        ],
        "Runner checkpoint boundary",
    );
    for source in [recovery, projection, daemon] {
        assert!(!source.contains("auto_resume_terminal"));
        assert!(!source.contains("issue_permit("));
        assert!(!source.contains("CapabilityBroker::new"));
    }
}

#[test]
fn corrupt_journal_does_not_boot_empty() {
    let eventlog = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let integrity = include_str!("../../kiana-eventlog/src/integrity.rs");
    let health = include_str!("../src/health.rs");
    let projection = include_str!("../src/projection.rs");
    let fixture = include_str!("../../kiana-core/tests/pd06_jsonl_guard.rs");

    require(
        eventlog,
        &[
            "eventlog_corrupt",
            "eventlog_committed_prefix_truncated",
            "eventlog_file_replaced",
            "load_delta",
            "read_from",
        ],
        "EventLog recovery",
    );
    require(
        integrity,
        &[
            "eventlog_integrity_unknown",
            "Corrupt",
            "Unknown",
            "quarantine",
            "scan_jsonl",
        ],
        "journal integrity",
    );
    require(
        health,
        &[
            "project_health_snapshot",
            "eventlog_capability_or_cursor_limit",
            "SignalStatus::Degraded",
            "effect_unknown_present",
        ],
        "readiness degradation",
    );
    require(
        projection,
        &[
            "run_projection_unsupported",
            "TerminalConflict",
            "run_terminal_conflict",
        ],
        "projection fail closed",
    );
    require(
        fixture,
        &[
            "eventlog_legacy_writer_after_upgrade",
            "eventlog_file_replaced",
            "CapabilityBroker",
        ],
        "corruption fixture",
    );
}

#[test]
fn rebuild_does_not_issue_permit() {
    let resources = include_str!("../src/resource_projection.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    let audit = include_str!("../src/audit_projection.rs");
    let recovery = include_str!("../src/recovery.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    require(
        resources,
        &[
            "project_recovery_resources",
            "recovery_resource_source_empty",
            "Read-only",
        ],
        "resource projection",
    );
    require(
        attempts,
        &[
            "project_capability_attempts",
            "CapabilityEffectState::Unknown",
            "fenced",
            "source_cursor",
        ],
        "attempt projection",
    );
    require(
        audit,
        &[
            "rebuild_audit_projection",
            "read_all_events",
            "AuditProjection",
            "checkpoint",
        ],
        "audit projection",
    );
    require(
        recovery,
        &[
            "recovery_resources",
            "read_all_events",
            "project_recovery_resources",
        ],
        "recovery projection",
    );
    require(
        ports,
        &[
            "ProjectionStorePort",
            "read_projection",
            "checkpoint",
            "projection_store_unsupported",
        ],
        "projection port",
    );
    for source in [resources, attempts, audit, recovery] {
        for forbidden in [
            "issue_permit(",
            "consume_approval(",
            "execute_authorized_request(",
            "CapabilityBroker",
        ] {
            assert!(
                !source.contains(forbidden),
                "restart rebuild widened authority: {forbidden}"
            );
        }
    }
}
