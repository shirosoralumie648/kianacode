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
        // 这一组名里带 "plan/control"：commit_swarm 与 handle_company_command
        // 都是控制面符号，在 core/swarm.rs；plan 侧只带域内符号。
        // 下面第 36 行本来就对 core 断过 commit_swarm，两处是自相矛盾的。
        assert!(
            plan.contains(marker) || core.contains(marker),
            "plan/control marker missing: {marker}"
        );
    }
    assert!(core.contains("commit_swarm"));
    assert!(core.contains("ExecutionStatus::ResultUnknown"));
}

#[test]
fn swarm_work_graph_is_validated_before_create_state_write() {
    let plan = include_str!("../../kiana-domain/src/swarm.rs");
    let create = plan
        .find("SwarmCommand::Create { plan }")
        .expect("Create transition branch");
    let validation = plan
        .find("graph.validate()")
        .expect("typed graph validation");
    let identity = plan
        .find("graph.swarm_plan_id.to_string()")
        .expect("typed graph identity check");
    let state_write = plan
        .find("next.swarms.insert(")
        .expect("Create state write");

    assert!(
        create < validation && validation < state_write,
        "WorkGraph validation must happen in the Create branch before state mutation"
    );
    assert!(
        create < identity && identity < state_write,
        "WorkGraph identity binding must happen in the Create branch before state mutation"
    );
}
