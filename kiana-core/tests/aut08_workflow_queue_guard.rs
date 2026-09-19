//! AUT-08 source guard for queue lease ownership and the single dispatch boundary.

#[test]
fn core_keeps_queue_lease_separate_from_capability_execution() {
    let source = include_str!("../src/workflow_queue.rs");
    let port = include_str!("../../kiana-ports/src/lib.rs");
    let adapter = include_str!("../../kiana-eventlog/src/workflow_queue.rs");
    for marker in [
        "validate_workflow_queue_dispatch",
        "workflow_queue_requires_recovery",
        "is_dispatchable",
        "WorkflowQueueLeaseStatus::ResultUnknown",
    ] {
        assert!(
            source.contains(marker),
            "AUT-08 core marker missing: {marker}"
        );
    }
    for marker in [
        "trait WorkflowQueueStore",
        "async fn heartbeat",
        "async fn fence",
        "async fn reclaim",
        "workflow_queue_claim_unsupported",
    ] {
        assert!(
            port.contains(marker),
            "AUT-08 port marker missing: {marker}"
        );
    }
    for marker in [
        "MemoryWorkflowQueueStore",
        "workflow_queue_reclaim_effect_in_flight",
        "workflow_queue_recovery_required",
        "tokio::sync::Mutex",
    ] {
        assert!(
            adapter.contains(marker),
            "AUT-08 adapter marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBrokerPort::execute"));
    assert!(!adapter.contains("CapabilityBrokerPort::execute"));
}
