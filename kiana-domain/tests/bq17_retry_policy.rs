//! BQ-17 deny-first fixtures for the retry classifier, bounded Retry-After and the per-attempt
//! reservation / cancellation fence.
//!
//! Denials run before the success path: a retry decision is never permission to send, and every
//! fixture is a pure value check that never calls a provider, broker or EventLog.

use kiana_domain::*;
use uuid::Uuid;

fn policy() -> RetryPolicy {
    RetryPolicy::new(3, 3, 5_000, 10_000).expect("policy")
}

fn error(code: &str, class: ModelRetryClass, sent: bool) -> ModelError {
    ModelError::transport(code, class, sent)
}

/// A typed rejection whose side-effect state is known to be `None`, i.e. the only post-send shape
/// the policy may retry (and only for 429/408).
fn no_effect_rejection(code: &str) -> ModelError {
    let mut error = ModelError::transport(code, ModelRetryClass::Rejected, true);
    error.side_effect_state = ModelSideEffectState::None;
    error
}

fn observation(error: &ModelError, observed_delta: bool, idempotent: bool) -> RetryObservation {
    RetryObservation::from_model_error(error, observed_delta, idempotent)
}

fn denial(
    policy: &RetryPolicy,
    attempt: u32,
    request_count: u32,
    now_unix_ms: u64,
    observation: &RetryObservation,
) -> RetryDenyReason {
    let decision = policy
        .classify(attempt, request_count, now_unix_ms, observation)
        .expect("decision");
    assert!(!decision.retry, "expected a denial");
    assert_eq!(decision.next_attempt, 0);
    assert_eq!(decision.delay_ms, 0);
    assert!(decision.validate().is_ok());
    decision.deny_reason.expect("deny reason")
}

fn reservation_with(
    retry_ordinal: u32,
    requested_tokens: u64,
    deadline_unix_ms: u64,
    capacity_lease_digest: Option<String>,
) -> Result<RetryAttemptReservation, String> {
    RetryAttemptReservation::new(
        QuotaReservationId::new(),
        RunId::new(),
        AttemptId::new(),
        retry_ordinal,
        requested_tokens,
        deadline_unix_ms,
        capacity_lease_digest,
    )
}

fn reservation() -> RetryAttemptReservation {
    reservation_with(1, 256, 10_000, None).expect("reservation")
}

fn resign(reservation: &mut RetryAttemptReservation) {
    reservation.reservation_digest = reservation.digest();
}

#[test]
fn classifier_refuses_permanent_transport_failures_and_post_send_unknown() {
    let policy = policy();

    // TLS handshake, authentication and arbitrary post-send failures are typed `Never` by the
    // provider boundary; the classifier refuses them before any budget or Retry-After grant.
    for (code, class, sent) in [
        ("provider_tls_failed", ModelRetryClass::Never, true),
        ("provider_auth_failed", ModelRetryClass::Never, true),
        ("provider_http_401", ModelRetryClass::Never, true),
        ("response_lost_after_send", ModelRetryClass::Never, true),
        ("eof_after_send", ModelRetryClass::Never, true),
    ] {
        assert_eq!(
            denial(
                &policy,
                1,
                1,
                1_000,
                &observation(&error(code, class, sent), false, true)
            ),
            RetryDenyReason::RecoveryDispositionNotRetryable,
            "{code} must stay terminal"
        );
    }

    // 503 is an explicit upstream rejection that is deliberately *not* in the retryable 429/408
    // set, even when the side-effect state is known to be `None`.
    assert_eq!(
        denial(
            &policy,
            1,
            1,
            1_000,
            &observation(&no_effect_rejection("provider_http_503"), false, true)
        ),
        RetryDenyReason::RequestAlreadySent
    );

    // A 429 whose effect state is unknown can never be auto-retried (post-send unknown).
    assert_eq!(
        denial(
            &policy,
            1,
            1,
            1_000,
            &observation(
                &error("provider_http_429", ModelRetryClass::Rejected, true),
                false,
                true
            )
        ),
        RetryDenyReason::SideEffectUnknown
    );

    // Declaring `BeforeSend` does not prove the request was not sent; `request_sent` wins.
    let mut claimed_pre_send = error("connection_refused", ModelRetryClass::BeforeSend, true);
    claimed_pre_send.side_effect_state = ModelSideEffectState::None;
    assert_eq!(
        denial(
            &policy,
            1,
            1,
            1_000,
            &observation(&claimed_pre_send, false, true)
        ),
        RetryDenyReason::RequestAlreadySent
    );

    // Channel drift (phase != "transport") is refused before classification.
    let mut wrong_phase = no_effect_rejection("provider_http_429");
    wrong_phase.phase = "stream".to_owned();
    assert_eq!(
        denial(
            &policy,
            1,
            1,
            1_000,
            &observation(&wrong_phase, false, true)
        ),
        RetryDenyReason::RecoveryDispositionNotRetryable
    );

    // Repair dispositions are not transport-retryable even when they carry a transport code.
    let repair = ModelError::invalid("model_format_repair_unavailable")
        .with_recovery_disposition(ModelRecoveryDisposition::FormatRepair);
    assert_eq!(
        denial(&policy, 1, 1, 1_000, &observation(&repair, false, true)),
        RetryDenyReason::RecoveryDispositionNotRetryable
    );

    // An observed delta or a non-idempotent observation is terminal for retry purposes.
    assert_eq!(
        denial(
            &policy,
            1,
            1,
            1_000,
            &observation(&no_effect_rejection("provider_http_429"), true, true)
        ),
        RetryDenyReason::ObservedDelta
    );
    assert_eq!(
        denial(
            &policy,
            1,
            1,
            1_000,
            &observation(&no_effect_rejection("provider_http_408"), false, false)
        ),
        RetryDenyReason::NonIdempotent
    );
}

#[test]
fn classifier_allows_only_known_no_effect_429_408_or_pre_send() {
    let policy = policy();
    let before_send = observation(
        &error("connection_refused", ModelRetryClass::BeforeSend, false),
        false,
        true,
    );
    let decision = policy
        .classify(1, 1, 1_000, &before_send)
        .expect("decision");
    assert!(decision.retry);
    assert_eq!(decision.next_attempt, 2);
    assert_eq!(decision.delay_ms, 100);
    assert_eq!(decision.deny_reason, None);
    assert!(decision.validate().is_ok());

    for code in ["provider_http_429", "provider_http_408"] {
        let decision = policy
            .classify(
                1,
                1,
                1_000,
                &observation(&no_effect_rejection(code), false, true),
            )
            .expect("decision");
        assert!(decision.retry, "{code} must be a bounded retry");
        assert_eq!(decision.next_attempt, 2);
        assert_eq!(decision.delay_ms, 100);
        assert_eq!(decision.deny_reason, None);
        assert!(decision.validate().is_ok());
    }

    for (code, class, sent) in [
        ("provider_http_503", ModelRetryClass::Rejected, true),
        ("provider_auth_failed", ModelRetryClass::Never, true),
        ("provider_tls_failed", ModelRetryClass::Never, true),
        ("response_lost_after_send", ModelRetryClass::Never, true),
    ] {
        let mut rejected = error(code, class, sent);
        if code == "provider_http_503" {
            rejected.side_effect_state = ModelSideEffectState::None;
        }
        let decision = policy
            .classify(1, 1, 1_000, &observation(&rejected, false, true))
            .expect("decision");
        assert!(!decision.retry, "{code} must not retry");
        assert!(decision.deny_reason.is_some());
    }
}

#[test]
fn classifier_fences_delta_unknown_non_idempotent_budget_and_retry_after() {
    let policy = policy();
    let mut rejected = no_effect_rejection("provider_http_429");
    rejected.retry_after_ms = Some(MAX_RETRY_AFTER_MS + 1);
    assert_eq!(
        denial(&policy, 1, 1, 1_000, &observation(&rejected, false, true)),
        RetryDenyReason::RetryAfterTooLarge
    );

    rejected.retry_after_ms = None;
    for (observed_delta, idempotent, side_effect) in [
        (true, true, ModelSideEffectState::None),
        (false, false, ModelSideEffectState::None),
        (false, true, ModelSideEffectState::Unknown),
    ] {
        let mut observation = observation(&rejected, observed_delta, idempotent);
        observation.side_effect_state = side_effect;
        assert!(
            !policy
                .classify(1, 1, 1_000, &observation)
                .expect("decision")
                .retry
        );
    }

    let allowed = observation(&no_effect_rejection("provider_http_429"), false, true);
    assert_eq!(
        denial(&policy, 3, 3, 1_000, &allowed),
        RetryDenyReason::AttemptBudget
    );
    assert_eq!(
        denial(&policy, 1, 3, 1_000, &allowed),
        RetryDenyReason::RequestBudget
    );
    assert_eq!(
        denial(&policy, 1, 1, 10_000, &allowed),
        RetryDenyReason::Deadline
    );
}

#[test]
fn refusal_reasons_are_stable_policy_codes() {
    for (reason, code) in [
        (
            RetryDenyReason::RecoveryDispositionNotRetryable,
            "recovery_disposition_not_retryable",
        ),
        (
            RetryDenyReason::RetryClassNotAllowed,
            "retry_class_not_allowed",
        ),
        (RetryDenyReason::RequestAlreadySent, "request_already_sent"),
        (RetryDenyReason::SideEffectUnknown, "side_effect_unknown"),
        (RetryDenyReason::ObservedDelta, "observed_delta"),
        (RetryDenyReason::NonIdempotent, "non_idempotent"),
        (RetryDenyReason::Cancelled, "cancelled"),
        (RetryDenyReason::AttemptBudget, "attempt_budget"),
        (RetryDenyReason::RequestBudget, "request_budget"),
        (RetryDenyReason::Deadline, "deadline"),
        (RetryDenyReason::RetryAfterTooLarge, "retry_after_too_large"),
    ] {
        assert_eq!(reason.as_str(), code);
    }
}

#[test]
fn retry_budget_and_deadline_refuse_before_scheduling_another_request() {
    let policy = policy();
    let retryable = no_effect_rejection("provider_http_429");
    let allowed = observation(&retryable, false, true);

    assert_eq!(
        denial(&policy, 0, 1, 1_000, &allowed),
        RetryDenyReason::AttemptBudget
    );
    assert_eq!(
        denial(&policy, 3, 1, 1_000, &allowed),
        RetryDenyReason::AttemptBudget
    );
    assert_eq!(
        denial(&policy, 1, 3, 1_000, &allowed),
        RetryDenyReason::RequestBudget
    );
    assert_eq!(
        denial(&policy, 1, 1, 10_000, &allowed),
        RetryDenyReason::Deadline
    );

    // The request budget can bind before the attempt budget; a fresh attempt still passes.
    // `max_requests` must cover `max_attempts`, so the cap is expressed on both budgets.
    let capped = RetryPolicy::new(2, 2, 5_000, 100_000).expect("policy");
    assert_eq!(
        denial(&capped, 1, 2, 1_000, &allowed),
        RetryDenyReason::RequestBudget
    );
    assert!(
        capped
            .classify(1, 1, 1_000, &allowed)
            .expect("decision")
            .retry
    );
}

#[test]
fn retry_after_above_the_bound_or_the_deadline_never_schedules_an_early_request() {
    let policy = policy();
    let mut rejected = no_effect_rejection("provider_http_429");

    rejected.retry_after_ms = Some(MAX_RETRY_AFTER_MS + 1);
    assert_eq!(
        denial(&policy, 1, 1, 1_000, &observation(&rejected, false, true)),
        RetryDenyReason::RetryAfterTooLarge
    );

    // A server delay that fits the absolute bound may still cross the shared deadline: the
    // decision is a denial, never an early request.
    rejected.retry_after_ms = Some(250);
    assert_eq!(
        denial(&policy, 1, 1, 9_950, &observation(&rejected, false, true)),
        RetryDenyReason::Deadline
    );
    assert_eq!(
        denial(&policy, 1, 1, 10_000, &observation(&rejected, false, true)),
        RetryDenyReason::Deadline
    );

    // Within the deadline the visible Retry-After is preserved as the delay.
    rejected.retry_after_ms = Some(900);
    let decision = policy
        .classify(2, 2, 1_000, &observation(&rejected, false, true))
        .expect("decision");
    assert!(decision.retry);
    assert_eq!(decision.next_attempt, 3);
    assert_eq!(decision.delay_ms, 900);
    assert!(decision.validate().is_ok());
}

#[test]
fn pre_send_backoff_is_bounded_growth_and_never_shortens_retry_after() {
    let policy = RetryPolicy::new(6, 6, 5_000, 200_000).expect("policy");
    let before_send = observation(
        &error("connection_refused", ModelRetryClass::BeforeSend, false),
        false,
        true,
    );

    let mut previous = 0;
    for (attempt, expected) in [(1_u32, 100_u64), (2, 200), (3, 400), (4, 800), (5, 1_600)] {
        let decision = policy
            .classify(attempt, attempt, 1_000, &before_send)
            .expect("decision");
        assert!(decision.retry);
        assert_eq!(decision.delay_ms, expected);
        assert!(decision.delay_ms > previous, "backoff must grow");
        previous = decision.delay_ms;
    }

    // max_backoff_ms caps exponential growth.
    let capped = RetryPolicy::new(6, 6, 150, 200_000).expect("policy");
    let decision = capped
        .classify(3, 3, 1_000, &before_send)
        .expect("decision");
    assert!(decision.retry);
    assert_eq!(decision.delay_ms, 150);
}

#[test]
fn policy_validation_refuses_unbounded_or_forged_policies_before_classification() {
    for (attempts, requests, backoff, deadline) in [
        (0, 3, 5_000, 10_000),
        (
            MAX_RETRY_ATTEMPTS + 1,
            MAX_RETRY_ATTEMPTS + 1,
            5_000,
            10_000,
        ),
        (3, 0, 5_000, 10_000),
        (3, 2, 5_000, 10_000),
        (3, 3, 0, 10_000),
        (3, 3, MAX_RETRY_AFTER_MS + 1, 10_000),
        (3, 3, 5_000, 0),
    ] {
        assert_eq!(
            RetryPolicy::new(attempts, requests, backoff, deadline).unwrap_err(),
            "retry_policy_invalid"
        );
    }

    let mut forged = policy();
    forged.policy_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(forged.validate().unwrap_err(), "retry_policy_invalid");
    let retryable = no_effect_rejection("provider_http_429");
    assert_eq!(
        forged
            .classify(1, 1, 1_000, &observation(&retryable, false, true))
            .unwrap_err(),
        "retry_policy_invalid"
    );

    let mut empty_phase = observation(&retryable, false, true);
    empty_phase.phase = "  ".to_owned();
    assert_eq!(
        empty_phase.validate().unwrap_err(),
        "retry_observation_phase_invalid"
    );
    let mut empty_code = observation(&retryable, false, true);
    empty_code.code = String::new();
    assert_eq!(
        empty_code.validate().unwrap_err(),
        "retry_observation_code_invalid"
    );
    let mut injected_phase = observation(&retryable, false, true);
    injected_phase.phase = "transport\nx".to_owned();
    assert_eq!(
        injected_phase.validate().unwrap_err(),
        "retry_observation_phase_invalid"
    );
    assert_eq!(
        policy().classify(1, 1, 1_000, &empty_code).unwrap_err(),
        "retry_observation_code_invalid"
    );
}

#[test]
fn forged_decision_shapes_are_refused_before_any_scheduling_use() {
    let forged_retry = RetryDecision {
        retry: true,
        next_attempt: 2,
        delay_ms: 100,
        deny_reason: Some(RetryDenyReason::Deadline),
    };
    assert_eq!(
        forged_retry.validate().unwrap_err(),
        "retry_decision_invalid"
    );

    let zero_delay_retry = RetryDecision {
        retry: true,
        next_attempt: 2,
        delay_ms: 0,
        deny_reason: None,
    };
    assert_eq!(
        zero_delay_retry.validate().unwrap_err(),
        "retry_decision_invalid"
    );

    let delayed_denial = RetryDecision {
        retry: false,
        next_attempt: 0,
        delay_ms: 5,
        deny_reason: Some(RetryDenyReason::Cancelled),
    };
    assert_eq!(
        delayed_denial.validate().unwrap_err(),
        "retry_denial_invalid"
    );

    let silent_denial = RetryDecision {
        retry: false,
        next_attempt: 0,
        delay_ms: 0,
        deny_reason: None,
    };
    assert_eq!(
        silent_denial.validate().unwrap_err(),
        "retry_denial_invalid"
    );
}

#[test]
fn retry_contracts_round_trip_and_reject_unknown_fields() {
    let policy = policy();
    let decoded: RetryPolicy =
        serde_json::from_value(serde_json::to_value(&policy).unwrap()).expect("policy round-trip");
    assert_eq!(decoded, policy);
    let mut extra = serde_json::to_value(&policy).unwrap();
    extra["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RetryPolicy>(extra).is_err());

    let observation = observation(&no_effect_rejection("provider_http_429"), false, true);
    let decoded: RetryObservation =
        serde_json::from_value(serde_json::to_value(&observation).unwrap())
            .expect("observation round-trip");
    assert_eq!(decoded, observation);
    let mut extra = serde_json::to_value(&observation).unwrap();
    extra["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RetryObservation>(extra).is_err());

    let mut reservation = reservation();
    reservation.dispatch().expect("dispatch");
    reservation.mark_unknown(None).expect("unknown");
    let decoded: RetryAttemptReservation =
        serde_json::from_value(serde_json::to_value(&reservation).unwrap())
            .expect("reservation round-trip");
    assert_eq!(decoded, reservation);
    assert!(decoded.validate().is_ok());
    let mut extra = serde_json::to_value(&reservation).unwrap();
    extra["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RetryAttemptReservation>(extra).is_err());
}

#[test]
fn bounded_retry_decisions_advance_one_request_per_attempt_and_stop_at_the_budget() {
    let policy = RetryPolicy::new(3, 4, 5_000, 100_000).expect("policy");
    let run_id = RunId::new();
    let mut attempt = 1_u32;
    let mut request_count = 0_u32;
    let mut now_unix_ms = 1_000_u64;
    let mut reservations = Vec::new();
    loop {
        let mut reservation = RetryAttemptReservation::new(
            QuotaReservationId::new(),
            run_id,
            AttemptId::new(),
            attempt,
            256,
            100_000,
            None,
        )
        .expect("reservation");
        reservation.dispatch().expect("dispatch");
        request_count = request_count.saturating_add(reservation.request_count);
        reservation.settle(Some(128)).expect("settle");
        reservations.push(reservation);

        let mut retryable = no_effect_rejection("provider_http_429");
        retryable.retry_after_ms = Some(250);
        let decision = policy
            .classify(
                attempt,
                request_count,
                now_unix_ms,
                &observation(&retryable, false, true),
            )
            .expect("decision");
        if !decision.retry {
            assert_eq!(decision.deny_reason, Some(RetryDenyReason::AttemptBudget));
            break;
        }
        assert!(decision.validate().is_ok());
        assert_eq!(decision.next_attempt, attempt + 1);
        assert_eq!(decision.delay_ms, 250);
        now_unix_ms += decision.delay_ms;
        attempt = decision.next_attempt;
    }

    // Three bounded attempts, exactly one dispatched request each: the ledger count matches the
    // requests the policy could have sent.
    assert_eq!(attempt, 3);
    assert_eq!(request_count, 3);
    assert_eq!(reservations.len(), 3);
    for reservation in &reservations {
        assert_eq!(reservation.state, RetryAttemptState::Settled);
        assert_eq!(reservation.request_count, 1);
        assert!(reservation.validate().is_ok());
    }
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
    assert_eq!(reservation.state, RetryAttemptState::Reserved);
    assert_eq!(reservation.revision, 1);

    reservation.dispatch().expect("dispatch");
    assert_eq!(reservation.request_count, 1);
    assert_eq!(reservation.state, RetryAttemptState::Sent);
    assert_eq!(reservation.revision, 2);

    reservation.settle(Some(128)).expect("settle");
    assert_eq!(reservation.request_count, 1);
    assert_eq!(reservation.state, RetryAttemptState::Settled);
    assert_eq!(reservation.observed_tokens, Some(128));
    assert_eq!(reservation.revision, 3);
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

#[test]
fn cancellation_race_refuses_a_second_terminal_state() {
    // Cancelled before dispatch: no request may ever be recorded afterwards.
    let mut cancelled = reservation();
    cancelled.cancel().expect("cancel");
    assert_eq!(
        cancelled.dispatch().unwrap_err(),
        "retry_attempt_dispatch_not_reserved"
    );
    assert_eq!(
        cancelled.settle(None).unwrap_err(),
        "retry_attempt_settle_state_invalid"
    );
    assert_eq!(
        cancelled.mark_unknown(None).unwrap_err(),
        "retry_attempt_unknown_state_invalid"
    );
    assert_eq!(
        cancelled.cancel().unwrap_err(),
        "retry_attempt_cancel_race_terminal"
    );
    assert_eq!(cancelled.state, RetryAttemptState::Cancelled);
    assert_eq!(cancelled.request_count, 0);

    // Settled after dispatch: a late cancellation or a second settlement is refused.
    let mut settled = reservation();
    settled.dispatch().expect("dispatch");
    settled.settle(Some(64)).expect("settle");
    assert_eq!(
        settled.cancel().unwrap_err(),
        "retry_attempt_cancel_race_terminal"
    );
    assert_eq!(
        settled.settle(Some(64)).unwrap_err(),
        "retry_attempt_settle_state_invalid"
    );
    assert_eq!(
        settled.mark_unknown(Some(64)).unwrap_err(),
        "retry_attempt_unknown_state_invalid"
    );
    assert_eq!(settled.state, RetryAttemptState::Settled);

    // Unknown after dispatch is terminal as well and keeps the conservative reservation record.
    let mut unknown = reservation();
    unknown.dispatch().expect("dispatch");
    unknown.mark_unknown(None).expect("unknown");
    assert_eq!(unknown.state, RetryAttemptState::Unknown);
    assert_eq!(
        unknown.settle(Some(1)).unwrap_err(),
        "retry_attempt_settle_state_invalid"
    );
    assert_eq!(
        unknown.cancel().unwrap_err(),
        "retry_attempt_cancel_race_terminal"
    );
    assert_eq!(unknown.request_count, 1);
}

#[test]
fn reservation_usage_cannot_exceed_the_reserved_upper_bound() {
    let mut over_bound = reservation();
    over_bound.dispatch().expect("dispatch");

    assert_eq!(
        over_bound.settle(Some(257)).unwrap_err(),
        "retry_attempt_usage_exceeds_reservation"
    );
    assert_eq!(over_bound.state, RetryAttemptState::Sent);
    assert_eq!(
        over_bound.mark_unknown(Some(257)).unwrap_err(),
        "retry_attempt_usage_exceeds_reservation"
    );
    assert_eq!(over_bound.state, RetryAttemptState::Sent);
    assert_eq!(over_bound.observed_tokens, None);

    // The exact upper bound settles; the observed value is within the reservation.
    over_bound.settle(Some(256)).expect("settle at the bound");
    assert_eq!(over_bound.observed_tokens, Some(256));

    let mut unknown = reservation();
    unknown.dispatch().expect("dispatch");
    unknown.mark_unknown(None).expect("unknown");
    assert_eq!(unknown.observed_tokens, None);
    assert_eq!(unknown.request_count, 1);
}

#[test]
fn reservation_identity_fields_and_digest_are_fenced() {
    assert_eq!(
        reservation_with(0, 256, 10_000, None).unwrap_err(),
        "retry_attempt_reservation_invalid"
    );
    assert_eq!(
        reservation_with(1, 0, 10_000, None).unwrap_err(),
        "retry_attempt_reservation_invalid"
    );
    assert_eq!(
        reservation_with(1, 256, 0, None).unwrap_err(),
        "retry_attempt_reservation_invalid"
    );
    assert_eq!(
        reservation_with(1, 256, 10_000, Some("not-a-digest".to_owned())).unwrap_err(),
        "retry_capacity_lease_digest_invalid"
    );
    assert!(reservation_with(1, 256, 10_000, Some(format!("sha256:{}", "a".repeat(64)))).is_ok());

    let mut tampered_digest = reservation();
    tampered_digest.reservation_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        tampered_digest.validate().unwrap_err(),
        "retry_attempt_reservation_invalid"
    );

    for nil in [0_u8, 1, 2] {
        let mut forged = reservation();
        match nil {
            0 => forged.reservation_id = QuotaReservationId::from_uuid(Uuid::nil()),
            1 => forged.run_id = RunId::from_uuid(Uuid::nil()),
            _ => forged.attempt_id = AttemptId::from_uuid(Uuid::nil()),
        }
        resign(&mut forged);
        assert_eq!(
            forged.validate().unwrap_err(),
            "retry_attempt_reservation_invalid"
        );
    }

    let mut forged_request_count = reservation();
    forged_request_count.request_count = 2;
    resign(&mut forged_request_count);
    assert_eq!(
        forged_request_count.validate().unwrap_err(),
        "retry_attempt_reservation_invalid"
    );

    let mut forged_state = reservation();
    forged_state.state = RetryAttemptState::Settled;
    resign(&mut forged_state);
    assert_eq!(
        forged_state.validate().unwrap_err(),
        "retry_attempt_reservation_invalid"
    );

    let mut forged_usage = reservation();
    forged_usage.observed_tokens = Some(999);
    resign(&mut forged_usage);
    assert_eq!(
        forged_usage.validate().unwrap_err(),
        "retry_attempt_reservation_invalid"
    );
}
