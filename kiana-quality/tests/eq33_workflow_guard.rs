#[test]
fn workflow_swarm_evaluator_stays_pure_and_bounded() {
    let source = include_str!("../src/workflow.rs");
    for forbidden in [
        "std::fs",
        "tokio::",
        "reqwest::",
        "Provider",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "Command::new",
        "TeamCreate",
        "SendMessage",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden workflow evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "WORKFLOW_SWARM_INPUT_SCHEMA",
        "WorkflowSwarmEvaluator",
        "workflow.dag_cycle",
        "workflow.attempt_invalid",
        "workflow.fanout_exceeded",
        "workflow.fanin_mismatch",
        "workflow.child_scope_expanded",
        "workflow.merge_missing",
        "workflow.compensation_missing",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-33 boundary marker: {required}"
        );
    }
}
