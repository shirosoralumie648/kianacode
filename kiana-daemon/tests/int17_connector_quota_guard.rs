#[test]
fn daemon_rechecks_quota_before_fixture_and_preserves_single_spine() {
    let daemon = include_str!("../src/connectors.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let core = include_str!("../../kiana-core/src/connector_quota.rs");
    let provider = include_str!("../../kiana-provider/src/connector_quota.rs");
    for marker in [
        "connector_quota_reservation",
        "connector_quota_claim",
        "connector_quota_policy",
        "validate_connector_quota_boundary",
        "connector_quota_effect_admission",
        "connector_quota_credential_generation_mismatch",
        "ConnectorQuotaReservation",
        "ConnectorQuotaSettlement",
        "ConnectorFixture",
        "provider_receipt",
        "append_idempotent_expected",
    ] {
        assert!(
            daemon.contains(marker)
                || broker.contains(marker)
                || core.contains(marker)
                || provider.contains(marker),
            "INT-17 daemon marker missing: {marker}"
        );
    }
    assert!(daemon.contains("fixture.find_case"));
    assert!(daemon.contains("validate_connector_quota_boundary"));
    assert!(!daemon.contains("tokio::spawn"));
    assert!(!daemon.contains("reqwest::Client"));
}
