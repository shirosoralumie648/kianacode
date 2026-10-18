#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::{compact_code, item_source};

#[test]
fn cleanup_contract_and_existing_supervisors_keep_unknown_isolated() {
    let domain = include_str!("../../kiana-domain/src/resource_cleanup.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    // stop_confirmed 这个 JSON 字段由调用方发出，不在 supervisor 自己的
    // 契约里——supervisor 用 ProcessGroupState/ProcessGuard 表达同一语义。
    let harness_caps = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let core = include_str!("../src/lib.rs");
    for marker in [
        "CleanupCause",
        "CleanupResourceKind",
        "IsolatedUnknown",
        "stop_confirmed",
        "effect_known",
        "safe_to_release_all",
        "ResourceRetention",
    ] {
        assert!(
            domain.contains(marker),
            "missing H33 cleanup marker: {marker}"
        );
    }
    for marker in ["RunCancellation", "unregister_in_flight", "checkpoint"] {
        assert!(
            runner.contains(marker),
            "missing runner cleanup marker: {marker}"
        );
    }
    for marker in ["kill_on_drop", "ProcessGuard", "Unknown"] {
        assert!(
            supervisor.contains(marker),
            "missing process cleanup marker: {marker}"
        );
    }
    assert!(
        harness_caps.contains("stop_confirmed"),
        "stop_confirmed evidence missing from the capability caller"
    );
    for marker in ["pub async fn shutdown", "flush", "shutdown_observability"] {
        assert!(
            daemon.contains(marker),
            "missing daemon shutdown marker: {marker}"
        );
    }
    for marker in ["release_builder_path_locks", "TerminalScopeGuard"] {
        assert!(
            core.contains(marker),
            "missing Core release marker: {marker}"
        );
    }
}

#[test]
fn cleanup_does_not_equate_host_drop_with_confirmed_stop() {
    let domain = include_str!("../../kiana-domain/src/resource_cleanup.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let observe = compact_code(&item_source(domain, "pub fn observe("));
    assert!(observe.contains("self.status=if(!self.requires_stop_confirmation||stop_confirmed)&&effect_known{CleanupResourceStatus::Released}else{CleanupResourceStatus::IsolatedUnknown}"));
    let validate = compact_code(&item_source(domain, "pub fn validate("));
    assert!(validate.contains("self.status==CleanupResourceStatus::Released"));
    assert!(validate
        .contains("self.requires_stop_confirmation&&!self.stop_confirmed||!self.effect_known"));
    assert!(validate.contains("cleanup_resource_invalid"));
    assert!(domain.contains("stop_confirmed"));
    assert!(supervisor.contains("ProcessGroupState::Unknown"));
    assert!(supervisor.contains("confirmed"));
}

#[test]
fn cleanup_producer_reads_owner_evidence_and_is_wired_to_run_end_and_shutdown() {
    let producer = include_str!("../src/resource_cleanup_producer.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");

    // 生产者必须是真实 owner 状态的读取者，而不是另一套执行循环。
    for marker in [
        "release_run_resources",
        "record_end_of_run_cleanup",
        "shutdown_in_flight_runs",
        "record_shutdown_log_failures",
        "check_resource_retention",
        "safe_to_release_all",
        "IsolatedUnknown",
    ] {
        assert!(
            producer.contains(marker),
            "missing H33 producer marker: {marker}"
        );
    }
    // 报告只写 resource_cleanup 聚合：它不能改写 run 终态，也不能伪装成 quarantine 事实。
    assert!(producer.contains("\"resource_cleanup\""));
    assert!(!producer.contains("\"resource_quarantined\""));
    assert!(!producer.contains("\"resource.released\""));

    // reducer/producer 不是 OS 级 killer：杀进程、发信号、删临时文件都必须留在 adapter/supervisor。
    for forbidden in [
        "std::process",
        "tokio::process",
        "libc::",
        "kill(",
        "Command::new",
    ] {
        assert!(
            !producer.contains(forbidden),
            "cleanup producer must not perform OS-level cleanup: {forbidden}"
        );
    }

    // 运行结束和关闭两条路径都必须经过生产者。
    assert!(
        lifecycle.contains("record_end_of_run_cleanup"),
        "run end must record cleanup evidence through the producer"
    );
    let shutdown = item_source(daemon, "pub async fn shutdown(");
    let cancel_at = shutdown
        .find("shutdown_in_flight_runs")
        .expect("shutdown must cancel and record in-flight runs");
    let close_at = shutdown
        .find("close_event_store")
        .expect("shutdown must close the event store");
    let log_failure_at = shutdown
        .find("record_shutdown_log_failures")
        .expect("shutdown must record observability log failure evidence");
    assert!(
        cancel_at < log_failure_at && log_failure_at < close_at,
        "cleanup evidence must be recorded before the event store closes"
    );
}
