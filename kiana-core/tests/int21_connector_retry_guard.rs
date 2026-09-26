#[test]
fn connector_retry_is_a_pure_bounded_classifier() {
    let retry = include_str!("../../kiana-domain/src/connector_retry.rs");
    let operation = include_str!("../../kiana-domain/src/connector_operation.rs");
    let reservation = include_str!("../../kiana-domain/src/connector_reservation.rs");
    let effect = include_str!("../../kiana-domain/src/effect_observation.rs");

    for marker in [
        "ConnectorRetryClass",
        "KnownNoEffect",
        "DeclaredIdempotent",
        "UnknownEffect",
        "ApprovalDenied",
        "EpochStale",
        "ScopeDenied",
        "ConnectorRetryPolicy",
        "max_attempts",
        "max_backoff_ms",
        "deadline_unix_ms",
        "next_attempt",
        "retry_after_ms",
        "idempotency_key_digest",
        "ConnectorRetryDecision",
        "connector_retry_observation_invalid",
    ] {
        assert!(
            retry.contains(marker)
                || operation.contains(marker)
                || reservation.contains(marker)
                || effect.contains(marker),
            "INT-21 marker missing: {marker}"
        );
    }

    assert!(retry.contains("effect_state == EffectObservationState::Unknown"));
    assert!(retry.contains("ConnectorRetryDenyReason::NonIdempotent"));
    assert!(retry.contains("ConnectorRetryDenyReason::RequestAlreadySent"));
    assert!(retry.contains("checked_add(1)"));
    assert!(retry.contains("fresh connector reservation/permit"));
    assert!(!retry.contains("CapabilityBroker"));
    assert!(!retry.contains("EventStorePort"));
    assert!(!retry.contains("authorize_and_execute"));
}
