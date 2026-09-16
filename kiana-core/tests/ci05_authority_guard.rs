#[test]
fn authority_ledger_is_event_replayable_and_epoch_fenced() {
    let domain = include_str!("../../kiana-domain/src/authority.rs");
    let core = include_str!("../src/authority.rs");
    let assignment = include_str!("../../kiana-domain/src/assignment.rs");
    for marker in [
        "pub struct PolicyProfile",
        "pub struct DataBoundary",
        "pub struct SharingGrant",
        "pub struct AuthorityLedger",
        "pub fn rebuild(events",
        "authority_event_version_gap_or_regression",
        "authority_event_kind_unknown",
        "authority_epoch_rollback",
        "sharing_operations",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "authority ledger marker missing: {marker}"
        );
    }
    for marker in [
        "AuthorityLedger::rebuild",
        "authority_ledger_invalid",
        "pub(crate) async fn authority_ledger",
        "pub(crate) async fn authority_epoch",
    ] {
        assert!(
            core.contains(marker),
            "core authority marker missing: {marker}"
        );
    }
    for marker in [
        "authority_epoch",
        "revoke_role",
        "assignment_expired_or_missing",
        "project_assignment_binding_mismatch",
    ] {
        assert!(
            assignment.contains(marker),
            "assignment fence marker missing: {marker}"
        );
    }
}
