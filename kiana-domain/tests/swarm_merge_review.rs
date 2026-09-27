use kiana_domain::{
    SwarmMergeReceipt, SwarmMergeReview, SwarmPartitionReview, SwarmReviewDecision,
    SWARM_MERGE_RECEIPT_SCHEMA, SWARM_MERGE_REVIEW_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn review(key: &str, author: &str, reviewer: &str) -> SwarmPartitionReview {
    let mut value = SwarmPartitionReview {
        partition_key: key.to_owned(),
        author_id: author.to_owned(),
        reviewer_id: reviewer.to_owned(),
        output_digest: D1.to_owned(),
        evidence_digest: D2.to_owned(),
        decision: SwarmReviewDecision::Accepted,
        reason: "independent review".to_owned(),
        review_digest: String::new(),
    };
    value.review_digest = kiana_domain::json_digest(&json!({
        "partition_key": value.partition_key,
        "author_id": value.author_id,
        "reviewer_id": value.reviewer_id,
        "output_digest": value.output_digest,
        "evidence_digest": value.evidence_digest,
        "decision": value.decision,
        "reason": value.reason,
    }));
    value
}

fn merge_review(reviews: Vec<SwarmPartitionReview>) -> SwarmMergeReview {
    let mut value = SwarmMergeReview {
        schema: SWARM_MERGE_REVIEW_SCHEMA.to_owned(),
        merge_digest: D1.to_owned(),
        expected_partition_keys: vec!["a".to_owned(), "b".to_owned()],
        reviews,
        policy_digest: D2.to_owned(),
        reviewer_epoch: 2,
        review_digest: String::new(),
    };
    value.review_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "merge_digest": value.merge_digest,
        "expected_partition_keys": value.expected_partition_keys,
        "reviews": value.reviews,
        "policy_digest": value.policy_digest,
        "reviewer_epoch": value.reviewer_epoch,
    }));
    value
}

#[test]
fn independent_one_decision_per_partition_and_company_acceptance_boundary() {
    let review = merge_review(vec![
        review("a", "author-a", "reviewer"),
        review("b", "author-b", "reviewer"),
    ]);
    review.validate().unwrap();
    let mut receipt = SwarmMergeReceipt {
        schema: SWARM_MERGE_RECEIPT_SCHEMA.to_owned(),
        merge_digest: D1.to_owned(),
        review_digest: review.review_digest.clone(),
        accepted_by: "acceptor".to_owned(),
        policy_digest: D2.to_owned(),
        conflict_refs: Vec::new(),
        company_acceptance_required: true,
        receipt_digest: String::new(),
    };
    receipt.receipt_digest = kiana_domain::json_digest(&json!({
        "schema": receipt.schema,
        "merge_digest": receipt.merge_digest,
        "review_digest": receipt.review_digest,
        "accepted_by": receipt.accepted_by,
        "policy_digest": receipt.policy_digest,
        "conflict_refs": receipt.conflict_refs,
        "company_acceptance_required": receipt.company_acceptance_required,
    }));
    receipt.validate().unwrap();
}

#[test]
fn self_review_duplicate_partition_and_unknown_fields_fail_closed() {
    let self_review = merge_review(vec![
        review("a", "same", "same"),
        review("b", "author-b", "reviewer"),
    ]);
    assert_eq!(
        self_review.validate().unwrap_err(),
        "swarm_partition_review_invalid"
    );

    let duplicate = merge_review(vec![
        review("a", "author-a", "reviewer"),
        review("a", "author-b", "reviewer"),
    ]);
    assert_eq!(
        duplicate.validate().unwrap_err(),
        "swarm_merge_review_partition_coverage_invalid"
    );

    let mut value = serde_json::to_value(merge_review(vec![
        review("a", "author-a", "reviewer"),
        review("b", "author-b", "reviewer"),
    ]))
    .unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmMergeReview>(value).is_err());
}
