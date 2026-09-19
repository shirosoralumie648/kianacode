#[test]
fn control_plane_keeps_advance_dispatch_on_the_existing_workflow_spine() {
    let source = include_str!("../src/automation.rs");
    assert!(source.contains("plan_command_intent"));
    assert!(source.contains("WorkflowEffect::Dispatch"));
    assert!(source.contains("authorize_and_execute"));
    assert!(!source.contains("tokio::spawn"));
}
