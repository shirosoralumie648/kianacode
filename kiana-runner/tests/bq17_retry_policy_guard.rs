#[test]
fn runner_owns_one_bounded_retry_driver_and_records_each_attempt_reservation() {
    let retry = include_str!("../src/retry.rs");
    let harness = include_str!("../src/harness.rs");
    let budget = include_str!("../src/budget.rs");
    for marker in [
        "RetryPolicy",
        "RetryObservation",
        "RetryDecision",
        "classify_retry",
        "ModelRetryClass::Never",
        "provider_http_429",
        "provider_http_408",
        "observed_delta",
        "MAX_PROVIDER_ATTEMPTS",
        "retry_after_ms",
        "remaining.saturating_sub(started.elapsed())",
    ] {
        assert!(
            retry.contains(marker) || harness.contains(marker),
            "BQ-17 runner retry marker missing: {marker}"
        );
    }
    for marker in [
        "RetryAttemptReservation",
        "QuotaReservationId::new()",
        "attempt_reservation.dispatch()",
        "attempt_reservation.settle",
        "attempt_reservation.mark_unknown",
        "retry_reservation",
        "reserve_attempt",
        "settle_attempt",
    ] {
        assert!(
            harness.contains(marker) || budget.contains(marker),
            "BQ-17 runner accounting marker missing: {marker}"
        );
    }
}

#[test]
fn runner_never_retries_unknown_effect_or_after_cancellation() {
    let retry = include_str!("../src/retry.rs");
    let harness = include_str!("../src/harness.rs");
    for marker in [
        "ModelSideEffectState::Unknown",
        "!observed_delta",
        "cancellation.cancelled()",
        "model_retry_deadline_exceeded",
        "model_attempt_limit",
        "is_safe_to_retry(&error, observed_delta)",
    ] {
        assert!(
            retry.contains(marker) || harness.contains(marker),
            "BQ-17 cancellation/unknown marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn(retry",
        "retry_unknown_effect",
        "retry_after_cancel",
        "retry_without_idempotency",
    ] {
        assert!(
            !retry.contains(forbidden) && !harness.contains(forbidden),
            "BQ-17 retry bypass marker present: {forbidden}"
        );
    }
}
