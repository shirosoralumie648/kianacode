#[test]
fn company_process_is_a_pure_adapter_over_the_existing_workflow_planner() {
    let domain = include_str!("../../kiana-domain/src/company_process.rs");
    let core_process = include_str!("../src/company_process.rs");
    let automation = include_str!("../src/automation.rs");
    let workflow = include_str!("../../kiana-workflow/src/durable.rs");

    for marker in [
        "COMPANY_PROCESS_SCHEMA",
        "COMPANY_PROCESS_TEMPLATE_SCHEMA",
        "COMPANY_PROCESS_INTENT_SCHEMA",
        "CompanyProcessNode",
        "CompanyProcessEvent",
        "CompanyProcessTemplate",
        "template_hash",
        "CompanyProcessState",
        "workflow_instance_id",
        "CompanyProcessIntent",
        "company_process_gate_not_open",
        "company_process_template_drift",
        "human_task",
        "plan_company_process",
        "plan_command_intent",
        "AutomationState",
        "WorkflowPlanIntent",
        "replayed",
    ] {
        assert!(
            domain.contains(marker)
                || core_process.contains(marker)
                || automation.contains(marker)
                || workflow.contains(marker),
            "CO-22 marker missing: {marker}"
        );
    }
    assert!(core_process.contains("plan_company_process"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("Runner"));
    assert!(!domain.contains("serde_json::from_str::<CapabilityRequest>"));
}
