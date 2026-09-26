#[test]
fn human_inbox_keeps_exact_task_binding_and_control_plane_decision_boundary() {
    let inbox = include_str!("../../kiana-domain/src/company_inbox.rs");
    let company_policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let platform = include_str!("../../kiana-domain/src/platform.rs");
    let core = include_str!("../src/company_inbox.rs");
    for marker in [
        "COMPANY_INBOX_CARD_SCHEMA",
        "CompanyInboxCard",
        "CompanyInboxLedger",
        "CompanyInboxCardKind",
        "company_inbox_stale_target",
        "company_inbox_wrong_decider",
        "company_inbox_double_consumption",
        "company_inbox_target_expired",
        "target_revision",
        "target_digest",
        "scope_digest",
        "HumanTask",
        "HumanInboxItem",
        "ControlPlane",
        "consume_company_inbox_card",
    ] {
        assert!(
            inbox.contains(marker)
                || company_policy.contains(marker)
                || platform.contains(marker)
                || core.contains(marker),
            "CO-39 marker missing: {marker}"
        );
    }
    for forbidden in [
        "auto_approve",
        "retry_unknown_effect",
        "Command::new",
        "ModelClient::new",
    ] {
        assert!(
            !inbox.contains(forbidden),
            "CO-39 bypass marker present: {forbidden}"
        );
    }
}
