use kiana_domain::*;
use serde_json::json;

fn swarm_event(
    swarm: SwarmPlanId,
    from: SwarmStatus,
    to: SwarmStatus,
    revision: u64,
    authority_epoch: u64,
    review_complete: bool,
) -> SwarmTransitionEvent {
    SwarmTransitionEvent::new(
        swarm,
        SwarmTransitionEntity::Swarm { from, to },
        revision,
        authority_epoch,
        RequestId::new(),
        None,
        review_complete,
    )
    .unwrap()
}

#[test]
fn reducer_rejects_illegal_terminal_unknown_and_merge_without_review() {
    let swarm = SwarmPlanId::new();
    assert_eq!(
        SwarmTransitionEvent::new(
            swarm,
            SwarmTransitionEntity::Swarm {
                from: SwarmStatus::Ready,
                to: SwarmStatus::Completed,
            },
            1,
            1,
            RequestId::new(),
            None,
            false,
        )
        .unwrap_err(),
        "swarm_transition_illegal"
    );

    let mut reducer = SwarmTransitionReducer::new(swarm, 1).unwrap();
    reducer
        .apply(&swarm_event(
            swarm,
            SwarmStatus::Reserved,
            SwarmStatus::Ready,
            1,
            1,
            false,
        ))
        .unwrap();
    reducer
        .apply(&swarm_event(
            swarm,
            SwarmStatus::Ready,
            SwarmStatus::Running,
            2,
            1,
            false,
        ))
        .unwrap();
    reducer
        .apply(&swarm_event(
            swarm,
            SwarmStatus::Running,
            SwarmStatus::ReadyToMerge,
            3,
            1,
            false,
        ))
        .unwrap();
    assert_eq!(
        SwarmTransitionEvent::new(
            swarm,
            SwarmTransitionEntity::Swarm {
                from: SwarmStatus::ReadyToMerge,
                to: SwarmStatus::Completed,
            },
            4,
            1,
            RequestId::new(),
            None,
            false,
        )
        .unwrap_err(),
        "swarm_transition_illegal"
    );
    reducer
        .apply(&swarm_event(
            swarm,
            SwarmStatus::ReadyToMerge,
            SwarmStatus::Completed,
            4,
            1,
            true,
        ))
        .unwrap();
    assert_eq!(reducer.swarm_status, SwarmStatus::Completed);

    assert_eq!(
        SwarmTransitionEvent::new(
            swarm,
            SwarmTransitionEntity::Swarm {
                from: SwarmStatus::Completed,
                to: SwarmStatus::Running,
            },
            5,
            1,
            RequestId::new(),
            None,
            true,
        )
        .unwrap_err(),
        "swarm_transition_illegal"
    );
    let partition = PartitionId::new();
    assert_eq!(
        SwarmTransitionEvent::new(
            swarm,
            SwarmTransitionEntity::Partition {
                partition_id: partition,
                from: PartitionStatus::ResultUnknown,
                to: PartitionStatus::Succeeded,
            },
            1,
            1,
            RequestId::new(),
            None,
            false,
        )
        .unwrap_err(),
        "swarm_partition_transition_illegal"
    );
}

#[test]
fn reducer_replay_matches_live_and_rejects_revision_epoch_regressions() {
    let swarm = SwarmPlanId::new();
    let partition = PartitionId::new();
    let attempt = AttemptId::new();
    let child = ChildCellId::new();
    let events = vec![
        swarm_event(
            swarm,
            SwarmStatus::Reserved,
            SwarmStatus::Ready,
            1,
            1,
            false,
        ),
        swarm_event(swarm, SwarmStatus::Ready, SwarmStatus::Running, 2, 1, false),
        SwarmTransitionEvent::new(
            swarm,
            SwarmTransitionEntity::Partition {
                partition_id: partition,
                from: PartitionStatus::Pending,
                to: PartitionStatus::Ready,
            },
            3,
            1,
            RequestId::new(),
            None,
            false,
        )
        .unwrap(),
        SwarmTransitionEvent::new(
            swarm,
            SwarmTransitionEntity::Attempt {
                partition_id: partition,
                attempt_id: attempt,
                child_cell_id: child,
                from: AttemptStatus::Proposed,
                to: AttemptStatus::Dispatched,
            },
            4,
            1,
            RequestId::new(),
            None,
            false,
        )
        .unwrap(),
    ];
    let mut live = SwarmTransitionReducer::new(swarm, 1).unwrap();
    for event in &events {
        live.apply(event).unwrap();
    }
    assert_eq!(
        SwarmTransitionReducer::replay(swarm, 1, &events).unwrap(),
        live
    );

    let gap = swarm_event(
        swarm,
        SwarmStatus::Running,
        SwarmStatus::ReadyToMerge,
        6,
        1,
        false,
    );
    assert_eq!(
        live.apply(&gap).unwrap_err(),
        "swarm_transition_revision_gap_or_regression"
    );

    let epoch_two = swarm_event(
        swarm,
        SwarmStatus::Running,
        SwarmStatus::ReadyToMerge,
        5,
        2,
        false,
    );
    live.apply(&epoch_two).unwrap();
    let stale_epoch = swarm_event(
        swarm,
        SwarmStatus::ReadyToMerge,
        SwarmStatus::Failed,
        6,
        1,
        false,
    );
    assert_eq!(
        live.apply(&stale_epoch).unwrap_err(),
        "swarm_transition_epoch_regression"
    );

    let encoded = serde_json::to_value(&events[0]).unwrap();
    let mut unknown = encoded;
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmTransitionEvent>(unknown).is_err());
}
