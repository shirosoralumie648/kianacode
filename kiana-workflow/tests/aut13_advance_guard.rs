#[test]
fn advance_planner_has_explicit_fanout_fanin_and_child_fences() {
    let source = include_str!("../src/durable.rs");
    for marker in [
        "MAX_WORKFLOW_FAN_OUT",
        "MAX_WORKFLOW_DEPTH",
        "ready_node_ids",
        "stable_fan_in_outputs",
        "workflow_fanin_result_missing",
        "workflow_child_definition_drift",
        "workflow_child_budget_exceeded",
        "workflow_child_depth_exceeded",
        "output_digest",
        "reconcile_subworkflow_children",
    ] {
        assert!(source.contains(marker), "AUT-13 marker missing: {marker}");
    }
    assert!(!source.contains("CapabilityBroker::new"));
    assert!(!source.contains("tokio::spawn"));
}
