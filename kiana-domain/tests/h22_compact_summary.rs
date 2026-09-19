use kiana_domain::{CompactSummary, ModelMessage};

#[test]
fn compact_summary_has_goal_pending_state_and_digest_without_forged_completion() {
    let messages = vec![
        ModelMessage::system("product rules"),
        ModelMessage::user("finish the migration"),
        ModelMessage::assistant_with_tools("I will inspect", Vec::new()),
        ModelMessage::tool("call-1", "pending observation"),
    ];
    let summary = CompactSummary::from_messages(&messages).unwrap();
    summary.validate().unwrap();
    assert_eq!(summary.goal, "finish the migration");
    assert_eq!(summary.pending_work.len(), 1);
    assert!(summary.completed_action_refs.is_empty());
    assert!(!summary.summary_digest.is_empty());

    let mut forged = summary;
    forged.completed_action_refs = vec!["completed:tool-call-1".to_owned()];
    assert_eq!(
        forged.validate().unwrap_err(),
        "compact_summary_evidence_ref_invalid"
    );
}
