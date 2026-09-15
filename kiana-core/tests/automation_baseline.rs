#[test]
fn automation_baseline_keeps_a_single_control_plane_execution_spine() {
    let core = include_str!("../src/automation.rs");
    let workflow = include_str!("../../kiana-workflow/src/durable.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let sdk = include_str!("../../kiana-entrypoints/src/sdk.rs");

    assert!(core.contains("plan_command"));
    assert!(core.contains("commit_workflow"));
    assert!(core.contains("authorize_and_execute"));
    assert!(!core.contains("tokio::spawn(async move {"));
    assert!(!workflow.contains("EventStore"));
    assert!(!workflow.contains("CapabilityBroker"));
    assert!(daemon.contains("handle_workflow_command"));
    assert!(daemon.contains("RequestBody::Command(command)"));
    assert!(sdk.contains("pub fn watch_scheduled_tasks"));
    assert!(sdk.contains("ScheduledTasksHandle"));
    assert_eq!(sdk.matches("watch_scheduled_tasks").count(), 1);
}

#[test]
fn legacy_scheduler_surface_is_marked_compatibility_only() {
    let roadmap = include_str!("../../docs/roadmap.md");
    let baseline = include_str!("../../docs/roadmap/automation-baseline.md");
    assert!(roadmap.contains("watch_scheduled_tasks"));
    assert!(baseline.contains("compatibility"));
    assert!(baseline.contains("second scheduler"));
    assert!(baseline.contains("proof-level"));
}
