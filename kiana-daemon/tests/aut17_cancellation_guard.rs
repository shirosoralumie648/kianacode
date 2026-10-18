//! AUT-17 daemon source guard：取消服务保持有界、单 worker、只经端口工作，且不建立第二条执行面。

#[test]
fn daemon_cancellation_service_is_bounded_and_has_no_second_effect_loop() {
    let daemon = include_str!("../src/lib.rs");
    let service = include_str!("../src/automation_cancellation.rs");
    for marker in [
        "automation_cancellation_service: AutomationCancellationService",
        "start_automation_cancellation_service",
        "automation_cancellation_service.shutdown()",
        "pub fn automation_cancellation_service(",
    ] {
        assert!(
            daemon.contains(marker),
            "AUT-17 daemon marker missing: {marker}"
        );
    }
    for marker in [
        "AUTOMATION_CANCELLATION_CHANNEL_CAPACITY",
        "mpsc::channel(AUTOMATION_CANCELLATION_CHANNEL_CAPACITY)",
        "tokio::spawn(run_worker",
        "receiver.close()",
        "CancellationServiceCommand::Shutdown",
        "AutomationCancellationStore",
        "AutomationWorkerStopController",
        "cancel_automation_worker_command",
        "record_automation_late_result",
        "automation_cancellation_service_not_started",
    ] {
        assert!(
            service.contains(marker),
            "AUT-17 service marker missing: {marker}"
        );
    }
    // 服务只经 store/取消 store/stop 端口工作：不得直接持有 EventStore、Broker、harness 或模型客户端。
    for forbidden in [
        "EventStorePort",
        "CapabilityBrokerPort",
        "CapabilityBroker::new",
        "KianaHarness",
        "ModelClient",
        "authorize_and_execute",
        "std::process::Command",
        "kiana_protocol",
        "kiana_client",
        "unsafe",
    ] {
        assert!(
            !service.contains(forbidden),
            "AUT-17 service crosses a boundary: {forbidden}"
        );
    }
    // 服务必须声明它不是 durable 证据。
    assert!(
        service.contains("没有 fsync") || service.contains("不构成 durable"),
        "AUT-17 daemon service must declare its proof ceiling"
    );
}

#[test]
fn aut17_daemon_fixtures_stay_wired_to_the_service_contract() {
    let service = include_str!("../src/automation_cancellation.rs");
    let fixtures = include_str!("aut17_cancellation.rs");
    for fixture in [
        "daemon_host_owns_the_cancellation_service_and_fails_closed_before_start",
        "queued_cancellation_is_not_executed_and_claimed_cancellation_stops_with_evidence",
        "unconfirmed_and_unknown_cancellations_are_counted_as_recovery",
        "late_results_are_fenced_and_never_counted_as_stopped",
    ] {
        assert!(
            fixtures.contains(&format!("async fn {fixture}()")),
            "AUT-17 daemon fixture missing: {fixture}"
        );
    }
    // 先拒绝在前：夹具必须断言 0/1 次 stop 与稳定拒绝码，且不得忽略测试。
    for marker in [
        "conflict(\"automation_cancel_execution_terminal\")",
        "conflict(\"automation_cancel_authority_epoch_stale\")",
        "conflict(\"automation_cancel_fact_unknown\")",
        "conflict(\"automation_cancel_already_fenced\")",
        "conflict(\"automation_cancellation_service_already_started\")",
        "port_unavailable:automation_cancellation_service_not_started",
        "report.recovery_required_count, 2",
        "report.recovery_required_count, 1",
        "report.not_executed_count, 1",
        "report.stopped_count, 1",
        "calls.load(Ordering::SeqCst), 0",
        "calls.load(Ordering::SeqCst), 1",
    ] {
        assert!(
            fixtures.contains(marker),
            "AUT-17 daemon fixture assertion missing: {marker}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));
    assert!(!service.contains("EventStore::append"));
}
