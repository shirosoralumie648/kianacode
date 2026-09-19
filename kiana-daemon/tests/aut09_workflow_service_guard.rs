//! AUT-09 source guard for the single DaemonHost-owned scheduler/worker service.

#[test]
fn daemon_service_is_bounded_and_does_not_create_an_effect_loop() {
    let daemon = include_str!("../src/lib.rs");
    let service = include_str!("../src/workflow_service.rs");
    for marker in [
        "workflow_service: WorkflowQueueService",
        "start_workflow_queue_service",
        "workflow_service.shutdown()",
        "WorkflowQueueStore",
    ] {
        assert!(
            daemon.contains(marker),
            "AUT-09 daemon marker missing: {marker}"
        );
    }
    for marker in [
        "WORKFLOW_SERVICE_CHANNEL_CAPACITY",
        "mpsc::channel(WORKFLOW_SERVICE_CHANNEL_CAPACITY)",
        "tokio::spawn(run_worker",
        "receiver.close()",
        "WorkflowQueueCommand::Shutdown",
        "ControlPlane/Broker spine",
    ] {
        assert!(
            service.contains(marker),
            "AUT-09 service marker missing: {marker}"
        );
    }
    assert!(!service.contains("CapabilityBrokerPort::execute"));
    assert!(!service.contains("KianaHarness::run"));
}
