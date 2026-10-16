#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::{compact_code, item_source, production_source};

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
        "derived_request_id(\"permit.consume\", id)",
    ] {
        assert!(
            pending.contains(marker) || permit.contains(marker) || dispatch.contains(marker),
            "SC-12 marker missing: {marker}"
        );
    }
    let consume = compact_code(&item_source(dispatch, "async fn verify_and_consume("));
    assert!(consume.contains("expected_versions:expected"));
    assert!(consume
        .contains("ifcommitted?{returnErr(dispatch_error(\"execution_permit_already_consumed\"))"));
    for forbidden in ["tokio::spawn", "reqwest", "TeamCreate", "SendMessage"] {
        assert!(
            !production_source(dispatch).contains(forbidden),
            "SC-12 execution bypass marker: {forbidden}"
        );
    }
}
