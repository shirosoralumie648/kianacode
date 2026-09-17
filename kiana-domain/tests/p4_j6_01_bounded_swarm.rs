use kiana_domain::*;

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
    .expect("partition")
}

fn graph(swarm: SwarmPlanId, partitions: Vec<Partition>) -> SwarmWorkGraph {
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
    .expect("bounded graph")
}

#[test]
fn swarm_fanout_is_bounded_and_merges_deterministically() {
    let swarm = SwarmPlanId::new();
    let first = partition(swarm, 0, "input-a", "project/a", "src/a.rs");
    let second = partition(swarm, 1, "input-b", "project/b", "src/b.rs");
    let graph = graph(swarm, vec![first.clone(), second.clone()]);
    let first_projection = graph.projection(1_001).expect("first projection");
    let second_projection = graph.projection(1_001).expect("replayed projection");
    assert_eq!(first_projection, second_projection);
    assert_eq!(first_projection.ready.len(), 2);
    assert!(graph.max_concurrency <= MAX_SWARM_CONCURRENCY);
    assert!(graph.max_partition_count <= MAX_SWARM_PARTITIONS as u32);
    assert!(graph.ttl_ms <= MAX_SWARM_TTL_MS);

    let mut completed = SwarmTransitionReducer::new(swarm, 1).expect("reducer");
    for (revision, (from, to, reviewed)) in [
        (SwarmStatus::Reserved, SwarmStatus::Ready, false),
        (SwarmStatus::Ready, SwarmStatus::Running, false),
        (SwarmStatus::Running, SwarmStatus::ReadyToMerge, false),
        (SwarmStatus::ReadyToMerge, SwarmStatus::Completed, true),
    ]
    .into_iter()
    .enumerate()
    {
        completed
            .apply(
                &SwarmTransitionEvent::new(
                    swarm,
                    SwarmTransitionEntity::Swarm { from, to },
                    revision as u64 + 1,
                    1,
                    RequestId::new(),
                    None,
                    reviewed,
                )
                .expect("legal bounded transition"),
            )
            .expect("apply transition");
    }
    assert_eq!(completed.swarm_status, SwarmStatus::Completed);
    assert_eq!(
        SwarmTransitionEvent::new(
            swarm,
            SwarmTransitionEntity::Swarm {
                from: SwarmStatus::ReadyToMerge,
                to: SwarmStatus::Completed,
            },
            5,
            1,
            RequestId::new(),
            None,
            false,
        )
        .unwrap_err(),
        "swarm_transition_illegal"
    );

    let overlapping = SwarmWorkGraph::new(
        swarm,
        vec![
            first,
            partition(swarm, 1, "input-c", "project/c", "src/a.rs"),
        ],
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
    .unwrap_err();
    assert_eq!(overlapping.code, "swarm_partition_overlap");
    let duplicate_input = SwarmWorkGraph::new(
        swarm,
        vec![
            partition(swarm, 0, "same", "project/d", "src/d.rs"),
            partition(swarm, 1, "same", "project/e", "src/e.rs"),
        ],
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
    .unwrap_err();
    assert_eq!(duplicate_input.code, "swarm_duplicate_fingerprint");
}
