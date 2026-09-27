use kiana_domain::{
    SwarmMergeDecision, SwarmMergePartition, SwarmMergePartitionResult, SwarmMergeStrategy,
    SwarmPlanId, SWARM_MERGE_REDUCER_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn partition(key: &str, result: SwarmMergePartitionResult) -> SwarmMergePartition {
    let mut value = SwarmMergePartition {
        partition_key: key.to_owned(),
        result,
        output_digest: (result == SwarmMergePartitionResult::Success).then(|| D1.to_owned()),
        evidence_digest: D2.to_owned(),
        partition_digest: String::new(),
    };
    value.partition_digest = kiana_domain::json_digest(&json!({
        "partition_key": value.partition_key,
        "result": value.result,
        "output_digest": value.output_digest,
        "evidence_digest": value.evidence_digest,
    }));
    value
}

fn decision(
    strategy: SwarmMergeStrategy,
    keys: Vec<&str>,
    partitions: Vec<SwarmMergePartition>,
    accepted: bool,
) -> SwarmMergeDecision {
    let mut value = SwarmMergeDecision {
        schema: SWARM_MERGE_REDUCER_SCHEMA.to_owned(),
        swarm_plan_id: SwarmPlanId::new(),
        strategy,
        expected_partition_keys: keys.into_iter().map(str::to_owned).collect(),
        partitions,
        policy_digest: (strategy == SwarmMergeStrategy::ExplicitPolicy).then(|| D1.to_owned()),
        acceptance_evidence_digest: (strategy == SwarmMergeStrategy::ExplicitPolicy)
            .then(|| D2.to_owned()),
        accepted,
        merge_digest: String::new(),
    };
    value.merge_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "swarm_plan_id": value.swarm_plan_id,
        "strategy": value.strategy,
        "expected_partition_keys": value.expected_partition_keys,
        "partitions": value.partitions,
        "policy_digest": value.policy_digest,
        "acceptance_evidence_digest": value.acceptance_evidence_digest,
        "accepted": value.accepted,
    }));
    value
}

#[test]
fn canonical_all_success_and_explicit_policy_are_replayable() {
    let all_success = decision(
        SwarmMergeStrategy::AllSuccess,
        vec!["a", "b"],
        vec![
            partition("a", SwarmMergePartitionResult::Success),
            partition("b", SwarmMergePartitionResult::Success),
        ],
        true,
    );
    all_success.validate().unwrap();

    let explicit = decision(
        SwarmMergeStrategy::ExplicitPolicy,
        vec!["a", "b"],
        vec![
            partition("a", SwarmMergePartitionResult::Success),
            partition("b", SwarmMergePartitionResult::Failed),
        ],
        true,
    );
    explicit.validate().unwrap();
}

#[test]
fn unordered_missing_unknown_and_first_success_shapes_fail_closed() {
    let unordered = decision(
        SwarmMergeStrategy::AllSuccess,
        vec!["a", "b"],
        vec![
            partition("b", SwarmMergePartitionResult::Success),
            partition("a", SwarmMergePartitionResult::Success),
        ],
        true,
    );
    assert_eq!(
        unordered.validate().unwrap_err(),
        "swarm_merge_partition_coverage_invalid"
    );

    let unknown = decision(
        SwarmMergeStrategy::AllSuccess,
        vec!["a"],
        vec![partition("a", SwarmMergePartitionResult::ResultUnknown)],
        true,
    );
    assert_eq!(
        unknown.validate().unwrap_err(),
        "swarm_merge_result_unknown_forbidden"
    );

    let mut value = serde_json::to_value(decision(
        SwarmMergeStrategy::AllSuccess,
        vec!["a"],
        vec![partition("a", SwarmMergePartitionResult::Success)],
        true,
    ))
    .unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmMergeDecision>(value).is_err());
}
