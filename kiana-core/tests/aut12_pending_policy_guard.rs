#[test]
fn control_plane_routes_trigger_policy_through_workflow_planner() {
    let source = include_str!("../src/automation.rs");
    assert!(source.contains("plan_command_intent"));
    assert!(source.contains("AutomationCommand"));
    assert!(!source.contains("tokio::spawn"));
    assert!(!source.contains("CapabilityBroker::new"));
}
