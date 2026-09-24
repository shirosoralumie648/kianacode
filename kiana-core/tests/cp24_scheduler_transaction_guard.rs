#[test]
fn cp24_scheduler_handoff_keeps_claim_lease_and_control_plane_boundaries() {
    let queue = include_str!("../src/workflow_queue.rs");
    let claim = include_str!("../../kiana-domain/src/workflow_queue_claim.rs");
    let packet = include_str!("../../kiana-domain/src/work_packets.rs");
    let lease = include_str!("../../kiana-domain/src/workflow_queue_lease.rs");
    let workflow = include_str!("../../kiana-workflow/src/durable.rs");
    let automation = include_str!("../src/automation.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let cells = include_str!("../src/cell_registry.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/cp24-scheduler-workflow-baseline.md");

    for marker in [
        "project_workflow_queue_ready",
        "validate_workflow_queue_transaction",
        "validate_workflow_queue_dispatch",
        "ready.validate()",
        "claim.validate_packet_binding",
        "WorkflowQueueStore",
        "WorkflowQueueLease",
        "fence_token",
        "authority_epoch",
        "workflow_queue_ready_snapshot_mismatch",
        "workflow_queue_lease_claim_mismatch",
        "handle_workflow_command",
        "plan_command_intent",
        "commit_workflow",
        "spawn_from_packet",
        "commit_spawn",
        "MAX_WORKFLOW_DEPTH",
        "MAX_WORKFLOW_FAN_OUT",
        "workflow_child_budget_exceeded",
        "workflow_child_definition_drift",
        "stable_fan_in_outputs",
        "spawn_depth_exceeded",
        "spawn_children_limit_exceeded",
        "spawn_grant_not_contained",
        "spawn_budget_not_contained",
    ] {
        assert!(
            queue.contains(marker)
                || claim.contains(marker)
                || packet.contains(marker)
                || lease.contains(marker)
                || workflow.contains(marker)
                || automation.contains(marker)
                || collaboration.contains(marker)
                || cells.contains(marker)
                || ports.contains(marker)
                || baseline.contains(marker),
            "CP-24 source marker missing: {marker}"
        );
    }

    assert!(queue.contains("ready.validate()?"));
    assert!(queue.contains("claim.validate_packet_binding(packet, parent)?"));
    assert!(queue.contains("validate_workflow_queue_dispatch("));
    assert!(automation.contains("plan_command_intent"));
    assert!(automation.contains("self.commit_workflow"));
    assert!(collaboration.contains("self.cell_registry.commit_spawn"));
    assert!(workflow.contains("stable_fan_in_outputs"));
    assert!(cells.contains("spawn_grant_not_contained"));
    assert!(ports.contains("trait WorkflowQueueStore"));

    for forbidden in [
        "CapabilityBroker::execute",
        "CapabilityBrokerPort::execute",
        "ModelClient",
        "KianaHarness",
        "tokio::spawn(async move {",
    ] {
        assert!(
            !queue.contains(forbidden),
            "queue bypass marker: {forbidden}"
        );
        assert!(
            !workflow.contains(forbidden),
            "workflow bypass marker: {forbidden}"
        );
    }
}
