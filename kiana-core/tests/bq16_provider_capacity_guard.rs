#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn bq16_capacity_contract_is_deny_first_and_control_plane_bound() {
    let domain = source_guard::production_source(include_str!(
        "../../kiana-domain/src/provider_capacity.rs"
    ));
    let provider_capacity =
        source_guard::production_source(include_str!("../../kiana-provider/src/capacity.rs"));
    let provider =
        source_guard::production_source(include_str!("../../kiana-provider/src/transport.rs"));
    let config =
        source_guard::production_source(include_str!("../../kiana-provider/src/config.rs"));
    for marker in [
        "ProviderCapacityOutcomeKind",
        "ProviderCapacityController",
        "ProviderCapacityLease",
        "CapacityWindow",
        "provider_capacity_quota_group_mismatch",
        "provider_capacity_queue_full",
        "provider_capacity_lease_owner_mismatch",
        "deadline_unix_ms",
        "provider_capacity_queue_wait_expired",
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
    let attempt = source_guard::item_source(&provider, "async fn send_inner_attempt(");
    let circuit = source_guard::compact_code(&source_guard::item_source(
        &provider,
        "async fn send_inner(",
    ));
    assert_eq!(
        circuit
            .matches("send_inner_attempt(connection,prepared,sink)")
            .count(),
        1
    );
    assert!(attempt.contains("try_acquire_owned"));
    assert!(attempt.contains("acquire_owned"));
    for forbidden in [
        "tokio::spawn",
        "fallback.unwrap()",
        "send_inner_attempt(connection, prepared",
        "provider_loop",
    ] {
        assert!(
            !if forbidden == "send_inner_attempt(connection, prepared" {
                attempt.contains(forbidden)
            } else {
                provider.contains(forbidden)
            },
            "BQ-16 provider boundary widened with forbidden marker: {forbidden}"
        );
    }
}
