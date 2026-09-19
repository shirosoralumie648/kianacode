//! CAP-12 source guard for the single process-tree supervisor and bounded resource evidence.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-12 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn shell_process_and_mcp_use_one_supervisor() {
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let mcp = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let domain = include_str!("../../kiana-domain/src/process_supervisor.rs");

    require(
        supervisor,
        &[
            "ProcessSupervisor",
            "prepare_command",
            "resource_receipt",
            "pub(crate) async fn stop",
            "StopReport",
            "ProcessGroupState",
            "RLIMIT_CPU",
            "RLIMIT_AS",
            "RLIMIT_NPROC",
            "TERM_GRACE",
            "KILL_GRACE",
            "wait_leader",
            "observe_process_group",
        ],
        "supervisor",
    );
    require(
        domain,
        &[
            "ProcessResourceBudget",
            "StopReport",
            "confirmed",
            "leader_reaped",
            "report_digest",
        ],
        "domain contract",
    );
    require(
        harness,
        &[
            "ProcessSupervisor::prepare_command",
            "ProcessSupervisor::stop",
            "stop_report",
        ],
        "shell adapter",
    );
    require(
        execution,
        &[
            "ProcessSupervisor::guard",
            "ProcessSupervisor::stop",
            "stop_report",
        ],
        "long-running process adapter",
    );
    require(
        mcp,
        &[
            "ProcessSupervisor::prepare_command",
            "ProcessSupervisor::stop",
        ],
        "MCP adapter",
    );
    for source in [harness, execution, mcp] {
        assert!(
            !source.contains("terminate_process_group"),
            "CAP-12 duplicate process stop helper remains"
        );
    }
}

#[test]
fn resource_receipt_does_not_overclaim_rlimit_as_or_nproc() {
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    require(
        supervisor,
        &[
            "per_process_rlimit_as",
            "per_process_rlimit_nproc",
            "rss",
            "observed_only",
            "aggregate_memory",
            "aggregate_pids",
            "not_hard_enforced",
        ],
        "resource receipt",
    );
}
