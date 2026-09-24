#[test]
fn bq16_capacity_contract_is_deny_first_and_control_plane_bound() {
    let domain = include_str!("../../kiana-domain/src/provider_capacity.rs");
    let provider_capacity = include_str!("../../kiana-provider/src/capacity.rs");
    let provider = include_str!("../../kiana-provider/src/transport.rs");
    let config = include_str!("../../kiana-provider/src/config.rs");
    for marker in [
        "ProviderCapacityOutcomeKind",
        "ProviderCapacityController",
        "ProviderCapacityLease",
        "CapacityWindow",
        "provider_capacity_quota_group_mismatch",
        "provider_capacity_queue_full",
        "provider_capacity_lease_owner_mismatch",
        "tokens_per_minute",
        "requests_per_minute",
        "try_acquire_owned",
        "acquire_owned",
        "ControlPlane",
    ] {
        assert!(
            domain.contains(marker)
                || provider_capacity.contains(marker)
                || provider.contains(marker)
                || config.contains(marker)
                || include_str!("../src/lib.rs").contains(marker),
            "BQ-16 source marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn",
        "fallback.unwrap()",
        "send_inner_attempt(connection, prepared",
        "provider_loop",
    ] {
        assert!(
            !provider.contains(forbidden),
            "BQ-16 provider boundary widened with forbidden marker: {forbidden}"
        );
    }
}
