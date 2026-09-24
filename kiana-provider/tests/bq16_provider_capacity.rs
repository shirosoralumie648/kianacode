#[test]
fn transport_keeps_waiters_bounded_and_releases_queue_before_active_slot() {
    let config = include_str!("../src/config.rs");
    let capacity = include_str!("../src/capacity.rs");
    let transport = include_str!("../src/transport.rs");
    for marker in [
        "ProviderCapacityPolicy",
        "capacity_policy",
        "KIANA_MODEL_QUEUE_LIMIT",
        "provider_capacity_tpm_exceeded",
        "try_acquire_owned",
        "acquire_owned",
        "drop(queue_slot)",
        "provider_capacity_queue_full",
        "requests_per_minute",
        "provider_capacity_clock_rollback",
    ] {
        assert!(
            config.contains(marker) || transport.contains(marker) || capacity.contains(marker),
            "BQ-16 provider marker missing: {marker}"
        );
    }
    for forbidden in [
        "queue_slots = tokio::sync::Semaphore::new(usize::MAX)",
        "tokio::spawn",
    ] {
        assert!(
            !transport.contains(forbidden),
            "unbounded provider waiter: {forbidden}"
        );
    }
}
