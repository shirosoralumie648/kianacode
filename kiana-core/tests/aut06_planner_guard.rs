#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn workflow_planner_intent_stays_pure_and_effects_return_to_control_plane() {
    let domain = include_str!("../../kiana-domain/src/automation.rs");
    let planner = include_str!("../../kiana-workflow/src/durable.rs");
    let core = include_str!("../src/automation.rs");
    for marker in [
        "WORKFLOW_PLAN_INTENT_SCHEMA",
        "WorkflowIntentKind",
        "WorkflowPlanIntent",
        "intent_digest",
        "definition_digest",
        "queue_key",
    ] {
        assert!(
            domain.contains(marker),
            "planner intent marker missing: {marker}"
        );
    }
    for marker in [
        "plan_command_intent",
        "derive_plan_intent",
        "WorkflowIntentKind::Dispatch",
        "WorkflowIntentKind::Wait",
        "WorkflowIntentKind::Terminal",
        "WorkflowIntentKind::Reserve",
        "WorkflowIntentKind::Noop",
    ] {
        assert!(planner.contains(marker), "planner marker missing: {marker}");
    }
    assert!(core.contains("plan_command_intent"));
    assert!(core.contains("commit_workflow"));
    // Forbidden markers are checked against the production view: planner docs may name the
    // EventStore boundary, but planner code must never reach for it.
    let planner_code = source_guard::production_source(planner);
    assert!(!planner_code.contains("EventStore"));
    assert!(!planner_code.contains("CapabilityBroker"));
    assert!(!planner_code.contains("SystemTime"));
    assert!(!planner_code.contains("tokio::spawn"));
}
