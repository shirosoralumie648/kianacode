//! AUT-15 daemon source guard：dispatch/observation 服务保持有界、单 worker、且不建立第二条执行面。

#[test]
fn daemon_dispatch_service_is_bounded_and_has_no_second_effect_loop() {
    let daemon = include_str!("../src/lib.rs");
    let service = include_str!("../src/automation_dispatch.rs");
    for marker in [
        "automation_dispatch_service: AutomationDispatchService",
        "start_automation_dispatch_service",
        "automation_dispatch_service.shutdown()",
        "pub fn automation_dispatch_service(",
    ] {
        assert!(
            daemon.contains(marker),
            "AUT-15 daemon marker missing: {marker}"
        );
    }
    for marker in [
        "AUTOMATION_DISPATCH_CHANNEL_CAPACITY",
        "mpsc::channel(AUTOMATION_DISPATCH_CHANNEL_CAPACITY)",
        "tokio::spawn(run_worker",
        "receiver.close()",
        "DispatchServiceCommand::Shutdown",
        "AutomationDispatchStore",
        "AutomationWorkerEffectRunner",
        "dispatch_automation_worker_command",
        "PortError::Unavailable(\"automation_dispatch_service_not_started\".to_owned())",
    ] {
        assert!(
            service.contains(marker),
            "AUT-15 service marker missing: {marker}"
        );
    }
    // 服务只经 store/runner 端口工作：不得直接持有 EventStore、Broker、harness 或模型客户端。
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
            "AUT-15 service crosses a boundary: {forbidden}"
        );
    }
}

#[test]
fn aut15_daemon_fixtures_stay_wired_to_the_service_contract() {
    let service = include_str!("../src/automation_dispatch.rs");
    let fixtures = include_str!("aut15_dispatch_observation.rs");
    for fixture in [
        "commit_and_dispatch_are_separate_phases_and_replays_never_rerun_the_effect",
        "a_claimed_command_replays_after_restart_without_a_second_effect",
        "unknown_results_and_observation_races_are_counted_as_recovery_only",
        "daemon_host_owns_the_service_and_shuts_it_down_with_the_spine",
    ] {
        assert!(
            fixtures.contains(&format!("async fn {fixture}()")),
            "AUT-15 daemon fixture missing: {fixture}"
        );
    }
    // 先拒绝在前：夹具必须断言 0/1 次 effect 与稳定拒绝码，且不得忽略测试。
    for marker in [
        "conflict(\"automation_dispatch_worker_conflict\")",
        "conflict(\"automation_dispatch_observation_conflict\")",
        "PortError::Conflict(reason.to_owned()).to_string()",
        "recovery_required_count: 1",
        "observed_count: 1",
        "calls.load(Ordering::SeqCst), 0",
        "calls.load(Ordering::SeqCst), 1",
    ] {
        assert!(
            fixtures.contains(marker),
            "AUT-15 daemon fixture assertion missing: {marker}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));
    assert!(!service.contains("EventStore::append"));
}
