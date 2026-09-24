#[test]
fn provider_rechecks_server_quota_and_capacity_without_dispatch() {
    let source = include_str!("../src/connector_quota.rs");
    let transport = include_str!("../src/transport.rs");
    for marker in [
        "connector_quota_effect_admission",
        "capacity_lease_digest",
        "connector_quota_capacity_lease_mismatch",
        "credential_generation",
        "ConnectorQuotaReservation",
        "ConnectorQuotaClaim",
        "ConnectorQuotaPolicy",
    ] {
        assert!(
            source.contains(marker) || transport.contains(marker),
            "INT-17 provider marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn",
        "reqwest::Client",
        "EventStore",
        "authorize_and_execute",
    ] {
        assert!(
            !source.contains(forbidden),
            "provider quota boundary must not dispatch or authorize: {forbidden}"
        );
    }
}
