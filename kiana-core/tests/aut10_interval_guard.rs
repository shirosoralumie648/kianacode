#[test]
fn control_plane_keeps_tick_in_the_existing_workflow_route() {
    let source = include_str!("../src/automation.rs");
    let workflow = include_str!("../../kiana-workflow/src/durable.rs");
    assert!(source.contains("plan_command_intent"));
    assert!(source.contains("AutomationCommand"));
    assert!(workflow.contains("plan_interval_due("));
    assert!(workflow.contains("AutomationCommand::Tick"));
    assert!(!source.contains("tokio::spawn"));
    assert!(!source.contains("CapabilityBroker::new"));
}
