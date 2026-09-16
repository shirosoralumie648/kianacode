use kiana_domain::*;
use serde_json::json;
use uuid::Uuid;

#[test]
fn swarm_ids_and_lineage_are_stable_and_reject_cross_swarm_partition() {
    let swarm = SwarmPlanId::new();
    let lineage = SwarmLineage::new(
        swarm,
        PartitionId::new(),
        ChildCellId::new(),
        AttemptId::new(),
        DispatchIntentId::new(),
        QueueEntryId::new(),
        MergeDecisionId::new(),
        Some("workflow-1".to_owned()),
        None,
        Some(RunId::new()),
        RequestId::new(),
        Some(EventId::new()),
        4,
        1,
    )
    .unwrap();
    lineage.validate().unwrap();
    lineage.validate_for_swarm(swarm).unwrap();
    assert_eq!(
        lineage.validate_for_swarm(SwarmPlanId::new()).unwrap_err(),
        "swarm_lineage_swarm_mismatch"
    );

    let encoded = serde_json::to_value(&lineage).unwrap();
    let decoded: SwarmLineage = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, lineage);
    let mut tampered = serde_json::to_value(&lineage).unwrap();
    tampered["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmLineage>(tampered).is_err());
}

#[test]
fn lineage_rejects_self_parent_and_zero_epoch() {
    let swarm = SwarmPlanId::new();
    assert_eq!(
        SwarmLineage::new(
            swarm,
            PartitionId::new(),
            ChildCellId::new(),
            AttemptId::new(),
            DispatchIntentId::new(),
            QueueEntryId::new(),
            MergeDecisionId::new(),
            None,
            Some(swarm),
            None,
            RequestId::new(),
            None,
            1,
            1,
        )
        .unwrap_err(),
        "swarm_lineage_self_parent"
    );

    let zero_epoch = SwarmLineage::new(
        swarm,
        PartitionId::new(),
        ChildCellId::new(),
        AttemptId::new(),
        DispatchIntentId::new(),
        QueueEntryId::new(),
        MergeDecisionId::new(),
        None,
        None,
        None,
        RequestId::new(),
        None,
        0,
        1,
    )
    .unwrap_err();
    assert_eq!(zero_epoch, "swarm_lineage_header_invalid");

    let mut empty_id = SwarmLineage::new(
        swarm,
        PartitionId::new(),
        ChildCellId::new(),
        AttemptId::new(),
        DispatchIntentId::new(),
        QueueEntryId::new(),
        MergeDecisionId::new(),
        None,
        None,
        None,
        RequestId::new(),
        None,
        1,
        1,
    )
    .unwrap();
    empty_id.partition_id = PartitionId::from_uuid(Uuid::nil());
    assert_eq!(empty_id.validate().unwrap_err(), "swarm_lineage_id_invalid");
}

#[test]
fn lineage_rejects_revision_and_epoch_regressions_and_digest_is_canonical() {
    let swarm = SwarmPlanId::new();
    let previous = SwarmLineage::new(
        swarm,
        PartitionId::new(),
        ChildCellId::new(),
        AttemptId::new(),
        DispatchIntentId::new(),
        QueueEntryId::new(),
        MergeDecisionId::new(),
        None,
        None,
        None,
        RequestId::new(),
        None,
        4,
        2,
    )
    .unwrap();
    let duplicate = previous.clone();
    assert_eq!(
        duplicate.validate_against(&previous).unwrap_err(),
        "swarm_lineage_revision_regression"
    );

    let stale_epoch = SwarmLineage::new(
        swarm,
        previous.partition_id,
        previous.child_cell_id,
        previous.attempt_id,
        previous.dispatch_intent_id,
        previous.queue_entry_id,
        previous.merge_decision_id,
        None,
        None,
        None,
        previous.correlation_id,
        None,
        3,
        3,
    )
    .unwrap();
    assert_eq!(
        stale_epoch.validate_against(&previous).unwrap_err(),
        "swarm_lineage_epoch_regression"
    );

    let first = json!({"b": 2, "a": 1});
    let second = json!({"a": 1, "b": 2});
    assert_eq!(json_digest(&first), json_digest(&second));
}
