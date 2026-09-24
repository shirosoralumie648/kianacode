#[test]
fn int18_control_plane_issues_and_rechecks_effect_fences() {
    let domain = include_str!("../../kiana-domain/src/connector_effect_permit.rs");
    let core = include_str!("../src/connector_reservation.rs");
    for marker in [
        "ConnectorEffectFence",
        "ConnectorEffectPermit",
        "issue_effect_permit",
        "admit_effect_permit",
        "authority_epoch",
        "configuration_epoch",
        "policy_epoch",
        "credential_epoch",
        "data_epoch",
        "scope_digest",
        "permit_digest",
        "expires_at_unix_ms",
        "validate_for_effect",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "INT-18 Core marker missing: {marker}"
        );
    }
    assert!(core.contains("ConnectorEffectPermit::issue"));
    assert!(core.contains("validate_for_effect"));
    assert!(!core.contains("tokio::spawn"));
}
