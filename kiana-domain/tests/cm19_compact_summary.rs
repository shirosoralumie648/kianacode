use kiana_domain::{
    CompactEvidenceKind, CompactEvidenceStatus, CompactSummary, CompactSummaryEvidence,
    ModelMessage,
};

fn digest(value: &str) -> String {
    kiana_domain::json_digest(&serde_json::json!({"value": value}))
}

#[test]
fn summary_cannot_forge_completed_tool_or_approval() {
    let messages = vec![ModelMessage::user("finish the migration")];
    let mut forged = CompactSummary::from_messages(&messages).unwrap();
    forged.completed_action_refs = vec!["event:tool-1".to_owned()];
    forged.summary_digest = forged.digest();
    assert_eq!(
        forged.validate().unwrap_err(),
        "compact_summary_completed_evidence_missing"
    );

    let pending = CompactSummaryEvidence::new(
        "event:tool-1",
        CompactEvidenceKind::CompletedAction,
        CompactEvidenceStatus::Pending,
        digest("pending-tool-fact"),
    )
    .unwrap();
    assert_eq!(
        forged.clone().with_evidence(vec![pending]).unwrap_err(),
        "compact_summary_completed_fact_not_confirmed"
    );

    let confirmed = CompactSummaryEvidence::new(
        "event:tool-1",
        CompactEvidenceKind::CompletedAction,
        CompactEvidenceStatus::Confirmed,
        digest("confirmed-tool-fact"),
    )
    .unwrap();
    let accepted = forged.with_evidence(vec![confirmed.clone()]).unwrap();
    accepted.validate_against_evidence(&[confirmed]).unwrap();

    let mut approval = CompactSummary::from_messages(&messages).unwrap();
    approval.decision_refs = vec!["event:approval-1".to_owned()];
    let denied = CompactSummaryEvidence::new(
        "event:approval-1",
        CompactEvidenceKind::Approval,
        CompactEvidenceStatus::Denied,
        digest("denied-approval-fact"),
    )
    .unwrap();
    assert_eq!(
        approval.with_evidence(vec![denied]).unwrap_err(),
        "compact_summary_decision_fact_not_confirmed"
    );
}
