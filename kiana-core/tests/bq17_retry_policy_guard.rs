#[test]
fn core_retry_projection_keeps_attempt_reservation_and_unknown_fences() {
    let domain = include_str!("../../kiana-domain/src/retry_policy.rs");
    let lifecycle = include_str!("../../kiana-domain/src/model_attempt_lifecycle.rs");
    let settlement = include_str!("../../kiana-domain/src/billing_settlement_fold.rs");
    let projection = include_str!("../src/model_attempt_projection.rs");
    for marker in [
        "RetryAttemptReservation",
        "QuotaReservationId",
        "request_count",
        "retry_ordinal",
        "retry_attempt_cancel_race_terminal",
        "ModelAttemptState::Unknown",
        "settlement",
    ] {
        assert!(
            domain.contains(marker)
                || lifecycle.contains(marker)
                || settlement.contains(marker)
                || projection.contains(marker),
            "BQ-17 core marker missing: {marker}"
        );
    }
    assert!(projection.contains("provider_http_408"));
    assert!(
        !projection.contains("provider_http_503") || projection.contains("ModelRetryClass::Never")
    );
}

#[test]
fn core_does_not_authorize_retry_or_create_provider_execution_loop() {
    let core = include_str!("../src/lib.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    for marker in ["ControlPlane", "CapabilityBrokerPort", "dispatch"] {
        assert!(
            core.contains(marker) || capabilities.contains(marker) || dispatch.contains(marker),
            "BQ-17 core authority marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn(provider_retry",
        "tokio::spawn(retry_loop",
        "send_inner_attempt(connection, prepared",
        "auto_retry_unknown",
    ] {
        assert!(
            !core.contains(forbidden)
                && !capabilities.contains(forbidden)
                && !dispatch.contains(forbidden),
            "BQ-17 second authority marker present: {forbidden}"
        );
    }
}
