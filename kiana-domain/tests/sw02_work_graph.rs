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
fn work_graph_rejects_noncanonical_wire_data_scope() {
    let swarm = SwarmPlanId::new();
    let left = partition(swarm, 0, "input-a", "project/a", "src/a.rs");
    let right = partition(swarm, 1, "input-b", " project/b ", "src/b.rs");
    assert_eq!(right.data_scope, vec!["project/b".to_owned()]);
    let valid = graph(swarm, vec![left.clone(), right]).unwrap();

    for scope in [" project/a/item", "project/a/item ", "\tproject/a/item\n"] {
        let mut untrusted = valid.clone();
        untrusted.partitions[1].data_scope = vec![scope.to_owned()];
        untrusted.graph_digest = untrusted.digest();
        assert_eq!(
            untrusted.partitions[1].validate().unwrap_err(),
            "swarm_partition_data_scope_invalid"
        );

        let wire = serde_json::to_value(&untrusted).unwrap();
        let decoded = serde_json::from_value::<SwarmWorkGraph>(wire).unwrap();
        let error = decoded.validate().unwrap_err();
        assert_eq!(error.code, "swarm_partition_data_scope_invalid");
        assert_eq!(
            error.partition_id,
            decoded.partitions[1].partition_id.to_string()
        );
        assert_eq!(decoded.projection(1_001).unwrap_err(), error);
    }

    let normalized = partition(swarm, 1, "input-b", " project/a/item ", "src/b.rs");
    assert_eq!(normalized.data_scope, vec!["project/a/item".to_owned()]);
    assert_eq!(
        graph(swarm, vec![left, normalized]).unwrap_err().code,
        "swarm_data_scope_overlap"
    );

    let wire = serde_json::to_value(&valid).unwrap();
    let decoded = serde_json::from_value::<SwarmWorkGraph>(wire).unwrap();
    decoded.validate().unwrap();
    assert_eq!(
        decoded.projection(1_001).unwrap(),
        valid.projection(1_001).unwrap()
    );
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
fn work_graph_rejects_duplicate_partition_keys_and_ordinals() {
    let swarm = SwarmPlanId::new();
    let first = partition(swarm, 0, "input-a", "project/a", "src/a.rs");
    let second = partition(swarm, 1, "input-b", "project/b", "src/b.rs");
    let valid = graph(swarm, vec![first, second]).unwrap();

    let mut duplicate_key = valid.clone();
    let duplicate_partition_id = duplicate_key.partitions[0].partition_id;
    duplicate_key.partitions[1].partition_id = duplicate_partition_id;
    duplicate_key.graph_digest = duplicate_key.digest();
    assert_eq!(
        duplicate_key.validate().unwrap_err().code,
        "swarm_partition_key_duplicate"
    );

    let mut duplicate_ordinal = valid;
    let duplicate_ordinal_value = duplicate_ordinal.partitions[0].ordinal;
    duplicate_ordinal.partitions[1].ordinal = duplicate_ordinal_value;
    duplicate_ordinal.graph_digest = duplicate_ordinal.digest();
    assert_eq!(
        duplicate_ordinal.validate().unwrap_err().code,
        "swarm_partition_ordinal_duplicate"
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

#[test]
fn work_graph_projection_reports_failure_causes_and_stably_sorts_ready_items() {
    let swarm = SwarmPlanId::new();
    let mut failed = partition(
        swarm,
        0,
        "input-failed",
        "project/failed",
        "src/failed.rs",
    );
    failed.status = PartitionStatus::Failed;
    let failed_id = failed.partition_id;

    let mut failed_dependent = partition(
        swarm,
        1,
        "input-failed-dependent",
        "project/failed-dependent",
        "src/failed-dependent.rs",
    );
    failed_dependent.dependency_partition_ids = vec![failed_id];
    let failed_dependent_id = failed_dependent.partition_id;

    let mut unknown = partition(
        swarm,
        2,
        "input-unknown",
        "project/unknown",
        "src/unknown.rs",
    );
    unknown.status = PartitionStatus::ResultUnknown;
    let unknown_id = unknown.partition_id;

    let mut cancelled = partition(
        swarm,
        3,
        "input-cancelled",
        "project/cancelled",
        "src/cancelled.rs",
    );
    cancelled.status = PartitionStatus::Cancelled;
    let cancelled_id = cancelled.partition_id;

    let pending_dependency = partition(
        swarm,
        4,
        "input-pending-dependency",
        "project/pending-dependency",
        "src/pending-dependency.rs",
    );
    let pending_dependency_id = pending_dependency.partition_id;
    let mut waiting = partition(swarm, 5, "input-waiting", "project/waiting", "src/waiting.rs");
    waiting.dependency_partition_ids = vec![pending_dependency_id];
    let waiting_id = waiting.partition_id;

    let mut running = partition(swarm, 6, "input-running", "project/running", "src/running.rs");
    running.status = PartitionStatus::Running;
    let running_id = running.partition_id;

    let mut succeeded = partition(
        swarm,
        7,
        "input-succeeded",
        "project/succeeded",
        "src/succeeded.rs",
    );
    succeeded.status = PartitionStatus::Succeeded;
    let mut after_succeeded = partition(
        swarm,
        8,
        "input-after-succeeded",
        "project/after-succeeded",
        "src/after-succeeded.rs",
    );
    after_succeeded.dependency_partition_ids = vec![succeeded.partition_id];
    let after_succeeded_id = after_succeeded.partition_id;

    let mut ready_items = vec![
        partition(swarm, 9, "input-ready-a", "project/ready-a", "src/ready-a.rs"),
        partition(swarm, 10, "input-ready-b", "project/ready-b", "src/ready-b.rs"),
        partition(swarm, 11, "input-ready-c", "project/ready-c", "src/ready-c.rs"),
    ];
    let mut expected_ready = vec![pending_dependency_id, after_succeeded_id];
    expected_ready.extend(ready_items.iter().map(|item| item.partition_id));
    expected_ready.sort_by_key(ToString::to_string);
    ready_items.sort_by_key(|item| item.partition_id.to_string());
    ready_items.reverse();

    let mut partitions = vec![
        failed,
        failed_dependent,
        unknown,
        cancelled,
        pending_dependency,
        waiting,
        running,
        succeeded,
        after_succeeded,
    ];
    partitions.extend(ready_items);

    let make_graph = |partitions| {
        SwarmWorkGraph::new(
            swarm,
            partitions,
            16,
            4,
            4,
            8,
            1_000,
            300_000,
            1_000,
            100,
            "all_success",
        )
    };
    let projection = make_graph(partitions.clone())
        .unwrap()
        .projection(1_001)
        .unwrap();
    partitions.reverse();
    let reordered_projection = make_graph(partitions)
        .unwrap()
        .projection(1_001)
        .unwrap();

    assert_eq!(projection, reordered_projection);
    assert_eq!(projection.ready, expected_ready);

    let mut expected_failed = vec![failed_id, failed_dependent_id, unknown_id, cancelled_id];
    expected_failed.sort_by_key(ToString::to_string);
    assert_eq!(projection.failed, expected_failed);

    assert_eq!(
        projection.blocked,
        BTreeMap::from([
            (
                failed_dependent_id.to_string(),
                format!("dependency_failed:{failed_id}"),
            ),
            (running_id.to_string(), "partition_running".to_owned()),
            (waiting_id.to_string(), "dependency_incomplete".to_owned()),
        ])
    );
}
