#[test]
fn approval_binding_is_exact_and_stays_before_broker_dispatch() {
    let binding = include_str!("../src/approval_binding.rs");
    let approvals = include_str!("../src/approvals.rs");
    for marker in [
        "ApprovalBinding",
        "target_digest",
        "payload_digest",
        "scope_digest",
        "PolicySelfApprovalForbidden",
        "PolicyApprovalBindingMismatch",
        "HumanInboxItem",
        "consume",
        "AUTH_PRINCIPAL_MISSING",
    ] {
        assert!(
            binding.contains(marker),
            "approval marker missing: {marker}"
        );
    }
    assert!(approvals.contains("prepare_capability_action"));
    assert!(approvals.contains("decide_approval_with_proof"));
    assert!(approvals.contains("validate_with_proof"));
    for forbidden in ["CapabilityBroker", "DaemonHost", "authorize_and_execute"] {
        assert!(
            !binding.contains(forbidden),
            "approval binding must not execute {forbidden}"
        );
    }
}
