//! Source guards for the EQ-43 quality mutation admission.
//!
//! These are structural assertions over the committed source, not runtime evidence: they catch a
//! refactor that quietly drops the authority recheck or replaces atomic approval consumption with
//! a plain append.

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
        "target_path",
    ] {
        assert!(
            event_contracts.contains(required),
            "missing quality event field: {required}"
        );
    }
    assert!(!gate.contains("CapabilityBroker"));
    assert!(!gate.contains("RunnerPort"));
}

#[test]
fn quality_mutation_consumes_approval_atomically_and_checks_target_scope() {
    let gate = include_str!("../src/quality_gate.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    // Consent must be spent, not merely read: the gate prepares the Approved -> Consumed
    // transition and commits it in the same batch as the admission fact.
    for required in [
        "prepare_consumption",
        "TransitionBatch",
        "commit_confirmed",
        "quality_approval_consumption_unavailable",
        "quality_mutation_conflict",
    ] {
        assert!(
            gate.contains(required),
            "missing atomic consumption marker: {required}"
        );
    }
    // The same command identity must be replayable from the committed receipt, so a retry is
    // answered rather than spending a second approval.
    for required in [
        "read_command",
        "derived_request_id",
        "quality_mutation_replay_",
    ] {
        assert!(gate.contains(required), "missing replay marker: {required}");
    }
    // The scope digest binds a target, but coverage is a separate server-side check.
    for required in ["allow_list_covers", "quality_target_outside_scope"] {
        assert!(
            gate.contains(required),
            "missing target scope marker: {required}"
        );
    }
    assert!(
        ports.contains("prepare_consumption"),
        "the approval port must expose atomic consumption"
    );
    // A single append would let the admission fact and the consumption diverge.
    assert!(
        !gate.contains("append_event(context.request_id, 1, &command, data"),
        "the admission fact must not be written through the single-event append path"
    );
}
