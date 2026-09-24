#[test]
fn provider_capacity_path_is_bounded_and_fallback_is_re_admitted() {
    let config = include_str!("../../kiana-provider/src/config.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let domain = include_str!("../../kiana-domain/src/provider_capacity.rs");
    for marker in [
        "KIANA_MODEL_QUEUE_LIMIT",
        "try_acquire_owned",
        "provider_capacity_queue_full",
        "ProviderCircuitBreaker",
        "provider_circuit_open",
        "FallbackRoutePlan",
        "admit_fallback",
    ] {
        assert!(
            config.contains(marker) || transport.contains(marker) || domain.contains(marker),
            "P4-J7-25 marker missing: {marker}"
        );
    }
    for forbidden in ["fallback.unwrap()", "tokio::spawn", "CapabilityBroker", "EventLog"] {
        assert!(
            !transport.contains(forbidden),
            "capacity transport boundary widened: {forbidden}"
        );
    }
}
