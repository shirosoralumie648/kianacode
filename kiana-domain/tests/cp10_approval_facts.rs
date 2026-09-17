use kiana_domain::{
    AggregateVersion, ApprovalConsumptionFact, ApprovalDecision, ApprovalDecisionFact, ApprovalId,
    RequestId, APPROVAL_CONSUMPTION_FACT_SCHEMA, APPROVAL_DECISION_FACT_SCHEMA,
};
use serde_json::json;

fn authority() -> AggregateVersion {
    AggregateVersion::new("authority", "project-authority", 7)
}

#[test]
fn approval_decision_and_consumption_are_distinct_strict_facts() {
    let approval_id = ApprovalId::new();
    let request_id = RequestId::new();
    let decision_command = RequestId::new();
    let dispatch_command = RequestId::new();
    let request_hash = format!("sha256:{}", "a".repeat(64));
    let decision = ApprovalDecisionFact::new(
        approval_id,
        request_id,
        request_hash.clone(),
        ApprovalDecision::Approve,
        "approver",
        decision_command,
        2,
        authority(),
        200,
        500,
    )
    .unwrap();
    assert_eq!(decision.schema, APPROVAL_DECISION_FACT_SCHEMA);
    assert!(decision.validate().is_ok());

    let consumption = ApprovalConsumptionFact::new(
        approval_id,
        request_id,
        request_hash,
        decision_command,
        dispatch_command,
        3,
        authority(),
        250,
        500,
    )
    .unwrap();
    assert_eq!(consumption.schema, APPROVAL_CONSUMPTION_FACT_SCHEMA);
    assert!(consumption.validate_after_decision(&decision).is_ok());
    assert_ne!(decision.decision_digest, consumption.consumption_digest);
}

#[test]
fn approval_facts_reject_replay_widening_and_unknown_fields() {
    let approval_id = ApprovalId::new();
    let request_id = RequestId::new();
    let decision = ApprovalDecisionFact::new(
        approval_id,
        request_id,
        format!("sha256:{}", "b".repeat(64)),
        ApprovalDecision::Deny,
        "approver",
        RequestId::new(),
        2,
        authority(),
        200,
        500,
    )
    .unwrap();
    let mut value = serde_json::to_value(&decision).unwrap();
    value["payload"] = json!({"secret":"must-not-be-carried"});
    assert!(serde_json::from_value::<ApprovalDecisionFact>(value).is_err());

    let mut tampered = serde_json::to_value(&decision).unwrap();
    tampered["expected_version"] = json!(99);
    let tampered: ApprovalDecisionFact = serde_json::from_value(tampered).unwrap();
    assert!(tampered.validate().is_err());
}
