#[test]
fn human_inbox_unifies_control_items_without_a_second_authority() {
    let platform = include_str!("../src/platform.rs");
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    let commands = include_str!("../src/commands.rs");
    let approvals = include_str!("../src/approvals.rs");
    let company = include_str!("../src/company.rs");
    let baseline = include_str!("../../docs/roadmap/p2-k3-01-human-inbox-baseline.md");
    for marker in [
        "human.inbox",
        "human.resolve",
        "HumanInboxItem",
        "HumanInboxKind::Approval",
        "HumanInboxKind::Review",
        "HumanInboxKind::Acceptance",
        "HumanInboxKind::Incident",
        "HumanInboxKind::Reconciliation",
        "HumanInboxKind::Feedback",
        "HumanAction",
        "pending_approvals",
        "company_business_inbox",
        "failure_incidents",
        "feedback_candidates",
        "company_revision",
        "inbox_revision",
        "human_inbox_stale",
        "human_item_not_found",
        "human_action_unavailable",
        "human_action_field_denied",
        "decide_approval_with_proof",
        "handle_company_command",
        "failure.reconcile",
        "feedback.review",
        "idempotency_key",
        "Human Inbox",
    ] {
        assert!(
            platform.contains(marker)
                || domain.contains(marker)
                || commands.contains(marker)
                || approvals.contains(marker)
                || company.contains(marker)
                || baseline.contains(marker),
            "human inbox marker missing: {marker}"
        );
    }
    assert!(platform.contains("items.sort_by"));
    assert!(platform.contains("json_digest(&json!(items))"));
    assert!(platform.contains("required_fields"));
    assert!(platform.contains("human_action_fields_required"));
    assert!(!platform.contains("CapabilityBroker"));
    assert!(!platform.contains("NotificationStore"));
    assert!(!platform.contains("DeliveryWorker"));
}
