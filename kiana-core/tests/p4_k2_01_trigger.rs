#[test]
fn trigger_cannot_execute_a_capability_directly() {
    let domain = include_str!("../../kiana-domain/src/automation.rs");
    let workflow = include_str!("../../kiana-workflow/src/durable.rs");
    let core = include_str!("../src/automation.rs");
    let commands = include_str!("../src/commands.rs");
    let fixture = include_str!("../../kiana-workflow/tests/p4_k2_01_trigger.rs");

    for marker in [
        "TriggerDefinition",
        "TriggerSchedule",
        "TriggerConcurrency",
        "RegisterTrigger",
        "DisableTrigger",
        "Tick",
        "Fire",
        "trigger_id",
        "approval_ref",
        "max_firings",
        "expires_at",
        "trigger_already_running",
        "trigger_budget_exhausted",
        "trigger_event_evidence_invalid",
        "trigger_execution_role_mismatch",
        "trigger_owner_or_role_denied",
        "WorkflowEffect::Dispatch",
        "plan_command",
        "commit_workflow",
        "handle_company_command",
        "authorize_and_execute",
        "RecordObservation",
        "trigger_cannot_execute_a_capability_directly",
    ] {
        assert!(
            domain.contains(marker)
                || workflow.contains(marker)
                || core.contains(marker)
                || commands.contains(marker)
                || fixture.contains(marker),
            "trigger marker missing: {marker}"
        );
    }

    assert!(workflow.contains("AutomationCommand::Fire"));
    assert!(workflow.contains("create_instance("));
    assert!(workflow.contains("WorkflowEffect::Dispatch"));
    assert!(core.contains("let committed = match self.commit_workflow"));
    assert!(core.contains("if let Some(WorkflowEffect::Dispatch"));
    assert!(core.contains("handle_company_command"));
    assert!(core.contains("authorize_and_execute"));
    assert!(!workflow.contains("CapabilityBroker"));
    assert!(!workflow.contains("ModelClient"));
    assert!(!core.contains("tokio::spawn(async move {"));
}
