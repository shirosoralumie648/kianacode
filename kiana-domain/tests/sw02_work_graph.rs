use kiana_domain::*;
use serde_json::json;
use std::collections::BTreeMap;

fn partition(swarm: SwarmPlanId, ordinal: u32, input: &str, scope: &str, path: &str) -> Partition {
    Partition::new(
        swarm,
        ordinal,
        vec![input.to_owned()],
        vec![scope.to_owned()],
        vec![path.to_owned()],
        "kiana.output.v1",
        1,
        Vec::new(),
        10,
        1,
    )
    .unwrap()
}

fn graph(
    swarm: SwarmPlanId,
    partitions: Vec<Partition>,
) -> Result<SwarmWorkGraph, SwarmGraphError> {
    SwarmWorkGraph::new(
        swarm,
        partitions,
        8,
        4,
        4,
        8,
        1_000,
        300_000,
        100,
        10,
        "all_success",
    )
}

#[test]
fn swarm_plan_rejects_partition_overlap_and_unbound_input() {
    let swarm = SwarmPlanId::new();
    let left = partition(swarm, 0, "input-a", "project/a", "src");
    let right = partition(swarm, 1, "input-b", "project/b", "src/lib.rs");
    assert_eq!(
        graph(swarm, vec![left, right]).unwrap_err().code,
        "swarm_partition_overlap"
    );

    let unbound = Partition::new(
        swarm,
        0,
        Vec::new(),
        vec!["project/c".to_owned()],
        vec!["docs/c.md".to_owned()],
        "kiana.output.v1",
        1,
        Vec::new(),
        10,
        1,
    )
    .unwrap_err();
    assert_eq!(unbound, "swarm_partition_input_unbound");
}

#[test]
fn work_graph_rejects_cycle_missing_duplicate_and_first_success() {
    let swarm = SwarmPlanId::new();
    let mut first = partition(swarm, 0, "input-a", "project/a", "src/a.rs");
    let mut second = partition(swarm, 1, "input-b", "project/b", "src/b.rs");
    first.dependency_partition_ids = vec![second.partition_id];
    second.dependency_partition_ids = vec![first.partition_id];
    let cycle = graph(swarm, vec![first.clone(), second.clone()]).unwrap_err();
    assert_eq!(cycle.code, "swarm_partition_dependency_cycle");
    assert_eq!(cycle.cycle.first(), cycle.cycle.last());

    let mut missing = partition(swarm, 0, "input-c", "project/c", "src/c.rs");
    missing.dependency_partition_ids = vec![PartitionId::new()];
    assert_eq!(
        graph(swarm, vec![missing]).unwrap_err().code,
        "swarm_partition_dependency_missing"
    );

    let duplicate = {
        let left = partition(swarm, 0, "same-input", "project/d", "src/d.rs");
        let right = partition(swarm, 1, "same-input", "project/e", "src/e.rs");
        graph(swarm, vec![left, right]).unwrap_err()
    };
    assert_eq!(duplicate.code, "swarm_duplicate_fingerprint");

    let first_success = graph(
        swarm,
        vec![partition(swarm, 0, "input-f", "project/f", "src/f.rs")],
    )
    .unwrap();
    let mut rejected = first_success;
    rejected.merge_strategy = "first_success".to_owned();
    rejected.graph_digest = rejected.digest();
    assert_eq!(
        rejected.validate().unwrap_err().code,
        "swarm_first_success_unsupported"
    );
}

#[test]
fn work_graph_rejects_limits_and_preserves_stable_projection() {
    let swarm = SwarmPlanId::new();
    let first = partition(swarm, 0, "input-a", "project/a", "src/a.rs");
    let second = partition(swarm, 1, "input-b", "project/b", "src/b.rs");
    let mut over_count = graph(swarm, vec![first.clone(), second.clone()]).unwrap();
    over_count.max_partition_count = 1;
    over_count.graph_digest = over_count.digest();
    assert_eq!(
        over_count.validate().unwrap_err().code,
        "swarm_partition_count_exceeded"
    );

    let mut over_concurrency = graph(swarm, vec![first.clone()]).unwrap();
    over_concurrency.max_concurrency = MAX_SWARM_CONCURRENCY + 1;
    over_concurrency.graph_digest = over_concurrency.digest();
    assert_eq!(
        over_concurrency.validate().unwrap_err().code,
        "swarm_concurrency_limit_exceeded"
    );

    let mut over_spawn_rate = graph(swarm, vec![first.clone()]).unwrap();
    over_spawn_rate.spawn_rate_limit = MAX_SWARM_SPAWN_RATE + 1;
    over_spawn_rate.graph_digest = over_spawn_rate.digest();
    assert_eq!(
        over_spawn_rate.validate().unwrap_err().code,
        "swarm_spawn_rate_limit_exceeded"
    );

    let mut over_ttl = graph(swarm, vec![first.clone()]).unwrap();
    over_ttl.ttl_ms = MAX_SWARM_TTL_MS + 1;
    over_ttl.graph_digest = over_ttl.digest();
    assert_eq!(
        over_ttl.validate().unwrap_err().code,
        "swarm_ttl_limit_exceeded"
    );

    let mut over_budget = graph(swarm, vec![first.clone(), second.clone()]).unwrap();
    over_budget.max_tokens = 10;
    over_budget.graph_digest = over_budget.digest();
    assert_eq!(
        over_budget.validate().unwrap_err().code,
        "swarm_token_budget_exceeded"
    );

    let mut chain_first = partition(swarm, 0, "input-c", "project/c", "src/c.rs");
    let chain_second = partition(swarm, 1, "input-d", "project/d", "src/d.rs");
    chain_first.dependency_partition_ids = vec![chain_second.partition_id];
    let mut over_depth = graph(swarm, vec![chain_first, chain_second]).unwrap();
    over_depth.max_depth = 1;
    over_depth.graph_digest = over_depth.digest();
    assert_eq!(
        over_depth.validate().unwrap_err().code,
        "swarm_depth_limit_exceeded"
    );

    let mut projection_first = partition(swarm, 0, "input-e", "project/e", "src/e.rs");
    let projection_second = partition(swarm, 1, "input-g", "project/g", "src/g.rs");
    projection_first.status = PartitionStatus::Succeeded;
    let mut projection = graph(swarm, vec![projection_first, projection_second]).unwrap();
    projection.graph_digest = projection.digest();
    let result = projection.projection(1_001).unwrap();
    assert_eq!(result.failed, Vec::<PartitionId>::new());
    assert_eq!(result.blocked, BTreeMap::new());
    assert_eq!(result.ready.len(), 1);
    assert_eq!(result.ready[0], projection.partitions[1].partition_id);

    let encoded = serde_json::to_value(&projection).unwrap();
    let mut unknown = encoded;
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmWorkGraph>(unknown).is_err());
}
