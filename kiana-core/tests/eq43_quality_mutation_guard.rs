#[test]
fn quality_mutation_uses_control_plane_secondary_authorization() {
    let commands = include_str!("../src/commands.rs");
    let gate = include_str!("../src/quality_gate.rs");
    let event_contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    for required in [
        "quality.promote",
        "quality.rollback",
        "handle_quality_mutation",
        "project_trusted",
        "ROLE_REVIEWER",
        "ROLE_QA",
    ] {
        assert!(
            commands.contains(required) || gate.contains(required),
            "missing route marker: {required}"
        );
    }
    for required in [
        "read_decision",
        "quality_secondary_approval_invalid",
        "quality_approval_request_hash_mismatch",
        "quality_scope_digest_mismatch",
        "quality_authority_epoch_stale",
        "authority_epoch",
        "append_event",
        "ApprovalState::Approved",
        "ApprovalDecision::Approve",
    ] {
        assert!(
            gate.contains(required),
            "missing secondary authorization marker: {required}"
        );
    }
    for required in [
        "candidate_digest",
        "gate_decision_digest",
        "approval_ref",
        "approval_request_hash",
        "scope_digest",
        "authority_epoch",
        "operation",
    ] {
        assert!(
            event_contracts.contains(required),
            "missing quality event field: {required}"
        );
    }
    assert!(!gate.contains("CapabilityBroker"));
    assert!(!gate.contains("RunnerPort"));
}
