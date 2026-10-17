//! BQ-17 runner source guard: one bounded retry driver, one reservation per attempt and a
//! cancellation fence around the backoff wait.
//!
//! The guard only pins source markers and negative boundaries; `include_str!` hits are not
//! runtime evidence.

#[test]
fn runner_owns_one_bounded_retry_driver_and_records_each_attempt_reservation() {
    let retry = include_str!("../src/retry.rs");
    let harness = include_str!("../src/harness.rs");
    let budget = include_str!("../src/budget.rs");
    let shared_retry = include_str!("../../kiana-domain/src/retry_policy.rs");
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
            retry.contains(marker) || harness.contains(marker) || shared_retry.contains(marker),
            "BQ-17 runner/shared retry marker missing: {marker}"
        );
    }
    assert!(retry.contains("RetryObservation::from_model_error"));
    assert!(retry.contains("policy.classify"));
    assert!(harness.contains("classify_retry("));
    // Exactly one classification site per attempt result; a second call site would mean a
    // second retry driver.
    assert_eq!(
        harness.matches("classify_retry(").count(),
        1,
        "BQ-17 must keep one retry classification site"
    );
    for marker in [
        "RetryAttemptReservation",
        "QuotaReservationId::new()",
        "attempt_reservation.dispatch()?;",
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
    // The per-attempt reservation and its bounded retry decision travel with the model turn
    // evidence; the settlement happens before the next attempt is decided.
    assert!(harness.contains("let retry_reservation = attempt_reservation.clone();"));
    assert!(harness.contains("\"retry_reservation\":retry_reservation,"));
    assert!(harness.contains("self.budget_ledger.settle_attempt(reservation, measured)?;"));
    assert!(harness.contains("attempt + 1 < MAX_PROVIDER_ATTEMPTS"));
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

#[test]
fn runner_backoff_wait_is_cancellable_and_deadline_bounded() {
    let retry = include_str!("../src/retry.rs");
    let harness = include_str!("../src/harness.rs");

    // The backoff sleep races the run cancellation; a cancelled run never sends the next
    // attempt, and a wait that no longer fits the shared deadline is refused explicitly.
    assert!(harness.contains(
        "tokio::select! {_=tokio::time::sleep(delay)=>{},error=cancellation.cancelled()=>return Err(error)}"
    ));
    assert!(harness.contains("return Err(\"model_retry_deadline_exceeded\".to_owned());"));
    assert!(harness.contains("let delay = retry_delay(&error, attempt, attempt_id);"));

    // The reserved upper bound and terminal state are runner-owned; the domain contracts come
    // from the shared crate and the budget ledger stays in-process accounting only.
    assert!(retry.contains("pub(crate) const MAX_PROVIDER_ATTEMPTS: u32 = 3;"));
    assert!(
        harness.contains("kiana_domain::RetryAttemptReservation")
            || harness.contains("RetryAttemptReservation")
    );
    for forbidden in [
        "auto_retry_unknown",
        "second_retry_driver",
        "tokio::spawn(async move { retry",
    ] {
        assert!(
            !retry.contains(forbidden) && !harness.contains(forbidden),
            "BQ-17 second retry driver marker present: {forbidden}"
        );
    }
    let ignore_attribute = format!("#[{}", "ignore");
    assert!(!retry.contains(&ignore_attribute));
    assert!(!harness.contains("retry_after_cancel"));
}
