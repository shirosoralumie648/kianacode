//! ER-25 behavioural fixtures for the bounded retry policy and new attempts.
//!
//! These fixtures drive the real `kiana_domain::RetryPolicy` classifier and the
//! `RetryAttemptReservation` value boundary — the same seam the Runner attempt loop and the
//! ControlPlane dispatch re-preflight consult. Nothing here restates the policy in a struct
//! field: every assertion reads the decision or the structured error the seam actually
//! returned.
//!
//! # Deny-first order
//!
//! The two deny fixtures run before the regression fixtures on purpose. An `Unknown` side
//! effect, an observed delta, a non-idempotent request, a policy-denied retry class and an
//! exhausted attempt/request/deadline budget must all refuse a new attempt. Only then do the
//! regression fixtures show that a pre-send transient, a known no-effect rejection and a
//! bounded backoff produce a *new*, bounded attempt.
//!
//! This is a source proof ceiling: it executes the shared classifier and the value boundary
//! in-process. It does not prove provider backoff over a real socket, cross-process retry
//! durability, or external effect reconciliation.

use kiana_domain::{
    AttemptId, ModelError, ModelRecoveryDisposition, ModelRetryClass, ModelSideEffectState,
    QuotaReservationId, RetryAttemptReservation, RetryAttemptState, RetryDenyReason,
    RetryObservation, RetryPolicy, RunId,
};

/// Fixed wall clock so the deadline arithmetic is deterministic and never depends on the host.
const NOW_UNIX_MS: u64 = 1_700_000_000_000;

fn bounded_policy() -> RetryPolicy {
    RetryPolicy::new(3, 6, 2_000, NOW_UNIX_MS + 60_000).expect("bounded retry policy")
}

fn observe(error: &ModelError, observed_delta: bool, idempotent: bool) -> RetryObservation {
    RetryObservation::from_model_error(error, observed_delta, idempotent)
}

/// An `Unknown` effect, an observed delta or a non-idempotent request never auto-retries.
#[test]
fn unknown_effect_never_auto_retries() {
    let policy = bounded_policy();

    // The request reached the provider and its result was lost, so the effect is Unknown.
    let lost = ModelError::transport(
        "provider_response_lost_after_send",
        ModelRetryClass::Rejected,
        true,
    );
    assert_eq!(lost.side_effect_state, ModelSideEffectState::Unknown);
    let decision = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&lost, false, true))
        .expect("classify");
    assert!(
        !decision.retry,
        "unknown effect must not auto-retry: {decision:?}"
    );
    assert_eq!(decision.deny_reason, Some(RetryDenyReason::SideEffectUnknown));
    assert_eq!(decision.next_attempt, 0);
    assert_eq!(decision.delay_ms, 0);
    decision.validate().expect("a denial is a well-formed decision");

    // A partial delta already reached the user; it can never be replayed as a fresh attempt.
    let pre_send = ModelError::transport("connect_reset", ModelRetryClass::BeforeSend, false);
    let observed = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&pre_send, true, true))
        .expect("classify");
    assert!(!observed.retry);
    assert_eq!(observed.deny_reason, Some(RetryDenyReason::ObservedDelta));

    // A non-idempotent request cannot be retried even when the failure is pre-send.
    let non_idempotent = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&pre_send, false, false))
        .expect("classify");
    assert!(!non_idempotent.retry);
    assert_eq!(
        non_idempotent.deny_reason,
        Some(RetryDenyReason::NonIdempotent)
    );
}

/// A policy-denied retry class or disposition never retries, even with budget left.
#[test]
fn policy_denied_never_retries() {
    let policy = bounded_policy();

    // A terminal (`Never`) transport failure is refused before the retry class is interpreted.
    let terminal = ModelError::transport("provider_http_503", ModelRetryClass::Never, true);
    let decision = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&terminal, false, true))
        .expect("classify");
    assert!(!decision.retry);
    assert_eq!(
        decision.deny_reason,
        Some(RetryDenyReason::RecoveryDispositionNotRetryable)
    );

    // A repair disposition (non-transport phase) is not a transport retry either.
    let repair = ModelError::invalid("format_rejected")
        .with_recovery_disposition(ModelRecoveryDisposition::FormatRepair);
    let decision = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&repair, false, true))
        .expect("classify");
    assert!(!decision.retry);
    assert_eq!(
        decision.deny_reason,
        Some(RetryDenyReason::RecoveryDispositionNotRetryable)
    );

    // A request that was already sent cannot be treated as a safe pre-send retry.
    let mut already_sent =
        ModelError::transport("connect_refused", ModelRetryClass::BeforeSend, false);
    already_sent.request_sent = true;
    let decision = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&already_sent, false, true))
        .expect("classify");
    assert!(!decision.retry);
    assert_eq!(
        decision.deny_reason,
        Some(RetryDenyReason::RequestAlreadySent)
    );

    // A rejected-but-never-sent class is not one of the two safe retry shapes.
    let rejected_unsent =
        ModelError::transport("provider_http_429", ModelRetryClass::Rejected, false);
    let decision = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&rejected_unsent, false, true))
        .expect("classify");
    assert!(!decision.retry);
    assert_eq!(
        decision.deny_reason,
        Some(RetryDenyReason::RetryClassNotAllowed)
    );
}

/// Attempt/request/deadline exhaustion is a terminal failure or Unknown, never a free retry.
#[test]
fn retry_exhaustion_is_terminal_failure_or_unknown() {
    let policy = bounded_policy();
    let transient = ModelError::transport("connect_reset", ModelRetryClass::BeforeSend, false);
    let observation = observe(&transient, false, true);

    // The last allowed attempt has no successor.
    let last = policy
        .classify(policy.max_attempts, 1, NOW_UNIX_MS, &observation)
        .expect("classify");
    assert!(!last.retry);
    assert_eq!(last.deny_reason, Some(RetryDenyReason::AttemptBudget));

    // Exhausting the request budget is terminal even for an otherwise retryable class.
    let requests = policy
        .classify(1, policy.max_requests, NOW_UNIX_MS, &observation)
        .expect("classify");
    assert!(!requests.retry);
    assert_eq!(requests.deny_reason, Some(RetryDenyReason::RequestBudget));

    // Past the absolute deadline a timeout is terminal, not a free retry.
    let timed_out = policy
        .classify(1, 1, NOW_UNIX_MS + 60_000, &observation)
        .expect("classify");
    assert!(!timed_out.retry);
    assert_eq!(timed_out.deny_reason, Some(RetryDenyReason::Deadline));
}

/// A pre-send transient creates a new, bounded attempt with its own backoff.
#[test]
fn transient_before_dispatch_creates_a_bounded_new_attempt() {
    let policy = bounded_policy();
    let error = ModelError::transport("connect_refused", ModelRetryClass::BeforeSend, false);
    let decision = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&error, false, true))
        .expect("classify");
    assert!(
        decision.retry,
        "pre-send transient must schedule a new attempt: {decision:?}"
    );
    assert_eq!(decision.next_attempt, 2);
    assert!(decision.delay_ms >= 100, "backoff must be positive");
    assert!(decision.delay_ms <= policy.max_backoff_ms);
    assert_eq!(decision.deny_reason, None);
    decision.validate().expect("a retry is a well-formed decision");
}

/// A known no-effect rejection retries under the bounded backoff and honors `Retry-After`.
#[test]
fn known_no_effect_failure_honors_retry_after_backoff() {
    let policy = bounded_policy();
    let mut refused = ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
    // A 429 is refused before any charge: the effect definitely did not happen.
    refused.side_effect_state = ModelSideEffectState::None;
    refused.retry_after_ms = Some(750);
    let decision = policy
        .classify(1, 1, NOW_UNIX_MS, &observe(&refused, false, true))
        .expect("classify");
    assert!(decision.retry, "{decision:?}");
    assert_eq!(decision.next_attempt, 2);
    assert!(
        decision.delay_ms >= 750,
        "backoff must honor Retry-After: {decision:?}"
    );
    assert!(decision.delay_ms <= policy.max_backoff_ms);
}

/// The absolute deadline bounds the backoff, so a new attempt is refused when it cannot fit.
#[test]
fn absolute_deadline_bounds_backoff_before_a_new_attempt() {
    let deadline = NOW_UNIX_MS + 1_000;
    let policy = RetryPolicy::new(3, 6, 2_000, deadline).expect("short-deadline policy");
    let mut refused = ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
    refused.side_effect_state = ModelSideEffectState::None;
    refused.retry_after_ms = Some(900);
    // Only 100ms remain, so the 900ms Retry-After backoff cannot fit before the deadline.
    let decision = policy
        .classify(
            1,
            1,
            deadline - 100,
            &observe(&refused, false, true),
        )
        .expect("classify");
    assert!(
        !decision.retry,
        "the deadline must bound the new attempt: {decision:?}"
    );
    assert_eq!(decision.deny_reason, Some(RetryDenyReason::Deadline));
}

/// A duplicate attempt is fenced at the per-attempt reservation: one send, one terminal.
#[test]
fn duplicate_idempotency_key_is_fenced_at_the_attempt_reservation() {
    let reservation = RetryAttemptReservation::new(
        QuotaReservationId::new(),
        RunId::new(),
        AttemptId::new(),
        1,
        128,
        NOW_UNIX_MS + 60_000,
        None,
    )
    .expect("retry attempt reservation");

    // A dispatched attempt is one bounded send; it cannot be cancelled into a duplicate.
    let mut sent = reservation.clone();
    sent.dispatch().expect("dispatch");
    assert_eq!(sent.state, RetryAttemptState::Sent);
    assert_eq!(
        sent.cancel().expect_err("a sent attempt cannot be cancelled"),
        "retry_attempt_cancel_race_terminal"
    );
    // Settling twice, or dispatching a settled attempt, is refused.
    sent.settle(Some(16)).expect("settle");
    assert_eq!(sent.state, RetryAttemptState::Settled);
    assert_eq!(
        sent.dispatch()
            .expect_err("a settled attempt cannot be sent again"),
        "retry_attempt_dispatch_not_reserved"
    );
    assert_eq!(
        sent.settle(None)
            .expect_err("a settled attempt cannot settle twice"),
        "retry_attempt_settle_state_invalid"
    );

    // A reserved attempt already satisfied elsewhere is cancelled, not re-dispatched.
    let mut reserved = reservation;
    reserved.cancel().expect("cancel a reserved attempt");
    assert_eq!(reserved.state, RetryAttemptState::Cancelled);
    assert_eq!(
        reserved
            .dispatch()
            .expect_err("a cancelled attempt cannot be dispatched"),
        "retry_attempt_dispatch_not_reserved"
    );
}
