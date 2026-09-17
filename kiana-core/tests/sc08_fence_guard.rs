#[test]
fn control_plane_fence_helpers_read_authority_and_never_revive_stale_work() {
    let fence = include_str!("../src/security_fence.rs");
    let domain = include_str!("../../kiana-domain/src/fencing.rs");
    let authority = include_str!("../src/authority.rs");
    for marker in [
        "issue_authority_fence",
        "validate_authority_fence",
        "authority_fence_snapshot",
        "authority_epoch",
        "authority_revision",
        "normalize_digest",
        "FACT_FENCE_MISMATCH",
    ] {
        assert!(
            fence.contains(marker),
            "fence helper marker missing: {marker}"
        );
    }
    for marker in [
        "AuthorityFence",
        "validate_successor",
        "validate_current",
        "PolicyAuthorityEpochRollback",
        "PolicyConfigRevisionStale",
        "UnknownFenceExpired",
        "parent_digest",
    ] {
        assert!(
            domain.contains(marker),
            "domain fence marker missing: {marker}"
        );
    }
    assert!(authority.contains("authority_epoch"));
    for forbidden in ["CapabilityBroker", "DaemonHost", "authorize_and_execute"] {
        assert!(
            !fence.contains(forbidden),
            "fence helper must not execute {forbidden}"
        );
    }
}
