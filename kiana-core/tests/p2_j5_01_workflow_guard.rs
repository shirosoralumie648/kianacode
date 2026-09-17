#[test]
fn workflow_definition_replay_uses_one_control_plane_spine() {
    let workflow = include_str!("../../kiana-workflow/src/durable.rs");
    let domain = include_str!("../../kiana-domain/src/automation.rs");
    let core = include_str!("../src/automation.rs");
    let baseline = include_str!("../../docs/roadmap/p2-j5-01-workflow-baseline.md");
    for marker in [
        "WorkflowDefinition",
        "WorkflowNodeExecution",
        "WorkflowInstanceStatus",
        "WorkflowEffect",
        "validate_definition",
        "definition_key",
        "workflow_definition_version_immutable",
        "workflow_input_digest",
        "WorkflowNodeKind::Approval",
        "WorkflowNodeKind::WaitSignal",
        "workflow_signal_evidence_required",
        "workflow_approval_evidence_required",
        "workflow_retry_denied",
        "workflow_compensation_requires_known_terminal",
        "plan_command",
        "commit_workflow",
        "load_workflows",
        "workflow_revision_conflict",
        "workflow_idempotency_conflict",
        "workflow_reconciliation_required",
        "authorize_and_execute",
        "EventStore",
        "ResultUnknown",
    ] {
        assert!(
            workflow.contains(marker)
                || domain.contains(marker)
                || core.contains(marker)
                || baseline.contains(marker),
            "workflow marker missing: {marker}"
        );
    }
    assert!(core.contains("self.commit_workflow"));
    assert!(core.contains("self.load_workflows"));
    assert!(core.contains("plan_command(&state"));
    assert!(core.contains("authorize_and_execute"));
    assert!(!workflow.contains("tokio::"));
    assert!(!workflow.contains("CapabilityBroker"));
    assert!(!workflow.contains("EventStore"));
    assert!(!core.contains("tokio::spawn(async move {"));
}
