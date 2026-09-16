#[test]
fn swarm_work_graph_uses_shared_packet_graph_and_keeps_execution_in_control_plane() {
    let domain = include_str!("../../kiana-domain/src/swarm_graph.rs");
    let packet_graph = include_str!("../../kiana-domain/src/packet_graph.rs");
    let plan = include_str!("../../kiana-domain/src/swarm.rs");
    let core = include_str!("../src/swarm.rs");
    for marker in [
        "pub struct Partition",
        "pub struct SwarmWorkGraph",
        "validate_dependency_graph",
        "swarm_partition_overlap",
        "swarm_partition_input_unbound",
        "swarm_first_success_unsupported",
        "swarm_depth_limit_exceeded",
        "swarm_spawn_rate_limit_exceeded",
        "pub fn projection",
    ] {
        assert!(
            domain.contains(marker),
            "work graph marker missing: {marker}"
        );
    }
    assert!(packet_graph.contains("pub fn validate_dependency_graph"));
    assert!(plan.contains("work_graph"));
    for marker in [
        "graph.validate()",
        "swarm_work_graph_invalid",
        "commit_swarm",
        "handle_company_command",
    ] {
        assert!(
            plan.contains(marker),
            "plan/control marker missing: {marker}"
        );
    }
    assert!(core.contains("commit_swarm"));
    assert!(core.contains("ExecutionStatus::ResultUnknown"));
}
