#[test]
fn action_card_is_shared_by_all_surfaces() {
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    let protocol_ui = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let core = include_str!("../src/platform.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let cli = include_str!("../../kiana-entrypoints/src/product_command.rs");
    let dispatch = include_str!("../../kiana-entrypoints/src/command_dispatch.rs");
    let baseline = include_str!("../../docs/roadmap/p2-m3-01-action-card-baseline.md");

    for marker in [
        "HumanAction",
        "HumanActionCard",
        "required_fields",
        "target_id",
        "expected_revision",
        "expected_epoch",
        "payload_digest",
        "allowed_decisions",
        "idempotency_key",
        "human.inbox",
        "human.resolve",
        "pending_approvals_envelope_on_host",
        "approval_decision_with_proof",
        "available_decisions",
        "failure.reconcile",
        "feedback.review",
        "company.business",
        "ui_action_stale",
        "human_inbox_stale",
        "human_action_field_denied",
        "human_action_fields_required",
        "human_action_unavailable",
        "approval_expired",
        "HumanInboxKind::Review",
        "HumanInboxKind::Acceptance",
        "HumanInboxKind::Incident",
    ] {
        assert!(
            domain.contains(marker)
                || protocol_ui.contains(marker)
                || protocol.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker)
                || web.contains(marker)
                || workbench.contains(marker)
                || cli.contains(marker)
                || dispatch.contains(marker)
                || baseline.contains(marker),
            "action-card marker missing: {marker}"
        );
    }

    assert!(core.contains("HumanAction"));
    assert!(protocol_ui.contains("HumanActionCard"));
    assert!(core.contains("human_action_field_denied"));
    assert!(core.contains("human_action_fields_required"));
    assert!(core.contains("human_inbox_stale"));
    assert!(workbench.contains("available_decisions"));
    assert!(web.contains("human.resolve"));
    assert!(cli.contains("RequestEnvelope::pending_approvals"));
    assert!(dispatch.contains("approval_decision_with_proof"));
    assert!(!core.contains("CapabilityBroker"));
}
