use kiana_domain::*;

fn policy() -> RetryPolicy {
    RetryPolicy::new(3, 3, 5_000, 10_000).expect("policy")
}

fn error(code: &str, class: ModelRetryClass, sent: bool) -> ModelError {
    ModelError::transport(code, class, sent)
}

#[test]
fn classifier_allows_only_known_no_effect_429_408_or_pre_send() {
    let policy = policy();
    let before_send = RetryObservation::from_model_error(
        &error("connection_refused", ModelRetryClass::BeforeSend, false),
        false,
        true,
    );
    assert!(
        policy
            .classify(1, 1, 1_000, &before_send)
            .expect("decision")
            .retry
    );

    for code in ["provider_http_429", "provider_http_408"] {
        let rejected = RetryObservation::from_model_error(
            &error(code, ModelRetryClass::Rejected, true),
            false,
            true,
        );
        assert!(
            policy
                .classify(1, 1, 1_000, &rejected)
                .expect("decision")
                .retry
        );
    }

    for (code, class, sent) in [
        ("provider_http_503", ModelRetryClass::Rejected, true),
        ("provider_auth_failed", ModelRetryClass::Never, true),
        ("provider_tls_failed", ModelRetryClass::Never, true),
        ("response_lost_after_send", ModelRetryClass::Never, true),
    ] {
        let denied = RetryObservation::from_model_error(&error(code, class, sent), false, true);
        assert!(
            !policy
                .classify(1, 1, 1_000, &denied)
                .expect("decision")
                .retry
        );
    }
}

#[test]
fn classifier_fences_delta_unknown_non_idempotent_budget_and_retry_after() {
    let policy = policy();
    let mut rejected = error("provider_http_429", ModelRetryClass::Rejected, true);
    rejected.side_effect_state = ModelSideEffectState::None;
    rejected.retry_after_ms = Some(MAX_RETRY_AFTER_MS + 1);
    let oversized = RetryObservation::from_model_error(&rejected, false, true);
    assert_eq!(
        policy
            .classify(1, 1, 1_000, &oversized)
            .expect("decision")
            .deny_reason,
        Some(RetryDenyReason::RetryAfterTooLarge)
    );

    for (observed_delta, idempotent, side_effect) in [
        (true, true, ModelSideEffectState::None),
        (false, false, ModelSideEffectState::None),
        (false, true, ModelSideEffectState::Unknown),
    ] {
        let mut observation = RetryObservation::from_model_error(
            &error("provider_http_429", ModelRetryClass::Rejected, true),
            observed_delta,
            idempotent,
        );
        observation.side_effect_state = side_effect;
        assert!(
            !policy
                .classify(1, 1, 1_000, &observation)
                .expect("decision")
                .retry
        );
    }

    let allowed = RetryObservation::from_model_error(
        &error("provider_http_429", ModelRetryClass::Rejected, true),
        false,
        true,
    );
    assert_eq!(
        policy
            .classify(3, 3, 1_000, &allowed)
            .expect("decision")
            .deny_reason,
        Some(RetryDenyReason::AttemptBudget)
    );
    assert_eq!(
        policy
            .classify(1, 3, 1_000, &allowed)
            .expect("decision")
            .deny_reason,
        Some(RetryDenyReason::RequestBudget)
    );
}

#[test]
fn attempt_reservation_has_one_request_and_one_terminal_state() {
    let run_id = RunId::new();
    let attempt_id = AttemptId::new();
    let lease = format!("sha256:{}", "a".repeat(64));
    let mut reservation = RetryAttemptReservation::new(
        QuotaReservationId::new(),
        run_id,
        attempt_id,
        1,
        256,
        10_000,
        Some(lease),
    )
    .expect("reservation");
    assert_eq!(reservation.request_count, 0);
    reservation.dispatch().expect("dispatch");
    reservation.settle(Some(128)).expect("settle");
    assert_eq!(reservation.request_count, 1);
    assert_eq!(reservation.state, RetryAttemptState::Settled);
    assert_eq!(
        reservation.cancel().unwrap_err(),
        "retry_attempt_cancel_race_terminal"
    );
    assert!(reservation.validate().is_ok());

    let mut cancelled = RetryAttemptReservation::new(
        QuotaReservationId::new(),
        run_id,
        AttemptId::new(),
        2,
        256,
        10_000,
        None,
    )
    .expect("reservation");
    cancelled.cancel().expect("cancel");
    assert_eq!(cancelled.state, RetryAttemptState::Cancelled);
    assert_eq!(cancelled.request_count, 0);
    assert!(cancelled.validate().is_ok());
}
