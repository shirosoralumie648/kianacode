#[test]
fn sc12_pending_invocation_permit_cas_and_idempotency_boundary_is_present() {
    let pending = include_str!("../../kiana-domain/src/capabilities.rs");
    let permit = include_str!("../../kiana-domain/src/dispatch.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    for marker in [
        "pub struct PendingInvocation",
        "event_sequence",
        "resume_binding",
        "pub struct DispatchPermit",
        "authority_versions",
        "permit_digest",
        "validate_for_request",
        "execution_permit_already_consumed",
        "old_epoch_permit_rejected",
        "expected_versions",
        "commit_transition",
        "idempotency",
    ] {
        assert!(
            pending.contains(marker) || permit.contains(marker) || dispatch.contains(marker),
            "SC-12 marker missing: {marker}"
        );
    }
    for forbidden in ["tokio::spawn", "reqwest", "TeamCreate", "SendMessage"] {
        assert!(
            !dispatch.contains(forbidden),
            "SC-12 execution bypass marker: {forbidden}"
        );
    }
}
