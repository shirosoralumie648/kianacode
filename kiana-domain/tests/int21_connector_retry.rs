use kiana_domain::{
    json_digest, ConnectorIdempotencyMode, ConnectorRetryClass, ConnectorRetryDenyReason,
    ConnectorRetryMode, ConnectorRetryObservation, ConnectorRetryPolicy, EffectObservationState,
};
use serde_json::json;

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn policy() -> ConnectorRetryPolicy {
    ConnectorRetryPolicy::new(4, 2_000, 10_000).expect("policy")
}

fn observation(
    class: ConnectorRetryClass,
    request_sent: bool,
    effect_state: EffectObservationState,
    declared_idempotent: bool,
    retry_after_ms: Option<u64>,
) -> ConnectorRetryObservation {
    ConnectorRetryObservation::new(
        class,
        request_sent,
        effect_state,
        declared_idempotent,
        declared_idempotent.then(|| digest("idempotency")),
        retry_after_ms,
        "connector_provider_error",
    )
    .expect("observation")
}

#[test]
fn known_no_effect_and_declared_idempotent_retry_with_new_attempt_and_bounded_delay() {
    let policy = policy();
    let known_no_effect = policy
        .classify(
            1,
            1_000,
            ConnectorRetryMode::KnownNoEffect,
            ConnectorIdempotencyMode::Optional,
            &observation(
                ConnectorRetryClass::KnownNoEffect,
                false,
                EffectObservationState::NoEffect,
                false,
                None,
            ),
        )
        .expect("known no effect decision");
    assert!(known_no_effect.retry);
    assert_eq!(known_no_effect.next_attempt, 2);
    assert_eq!(known_no_effect.delay_ms, 100);
    assert_eq!(known_no_effect.validate(), Ok(()));

    let declared = policy
        .classify(
            2,
            1_000,
            ConnectorRetryMode::DeclaredIdempotent,
            ConnectorIdempotencyMode::Required,
            &observation(
                ConnectorRetryClass::ProviderThrottled,
                true,
                EffectObservationState::NoEffect,
                true,
                Some(700),
            ),
        )
        .expect("declared idempotent decision");
    assert!(declared.retry);
    assert_eq!(declared.next_attempt, 3);
    assert_eq!(declared.delay_ms, 700);
    assert_eq!(declared.policy_digest, policy.policy_digest);
}

#[test]
fn unknown_side_effect_and_authority_failures_never_retry() {
    let policy = policy();
    for (class, state, reason) in [
        (
            ConnectorRetryClass::Unknown,
            EffectObservationState::Unknown,
            ConnectorRetryDenyReason::UnknownEffect,
        ),
        (
            ConnectorRetryClass::ApprovalDenied,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::ApprovalDenied,
        ),
        (
            ConnectorRetryClass::EpochStale,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::EpochStale,
        ),
        (
            ConnectorRetryClass::ScopeDenied,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::ScopeDenied,
        ),
        (
            ConnectorRetryClass::ValidationFailed,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::ValidationFailed,
        ),
    ] {
        let decision = policy
            .classify(
                1,
                1_000,
                ConnectorRetryMode::DeclaredIdempotent,
                ConnectorIdempotencyMode::Required,
                &observation(class, false, state, true, None),
            )
            .expect("deny decision");
        assert!(!decision.retry);
        assert_eq!(decision.deny_reason, Some(reason));
    }
}

#[test]
fn non_idempotent_request_budget_deadline_and_tamper_fail_closed() {
    let policy = policy();
    let non_idempotent = policy
        .classify(
            1,
            1_000,
            ConnectorRetryMode::DeclaredIdempotent,
            ConnectorIdempotencyMode::Forbidden,
            &observation(
                ConnectorRetryClass::ProviderFailed,
                true,
                EffectObservationState::NoEffect,
                false,
                None,
            ),
        )
        .expect("non-idempotent deny");
    assert_eq!(
        non_idempotent.deny_reason,
        Some(ConnectorRetryDenyReason::NonIdempotent)
    );

    let attempt_budget = policy
        .classify(
            4,
            1_000,
            ConnectorRetryMode::KnownNoEffect,
            ConnectorIdempotencyMode::Optional,
            &observation(
                ConnectorRetryClass::KnownNoEffect,
                false,
                EffectObservationState::NoEffect,
                false,
                None,
            ),
        )
        .expect("attempt budget deny");
    assert_eq!(
        attempt_budget.deny_reason,
        Some(ConnectorRetryDenyReason::AttemptBudget)
    );

    let deadline = policy
        .classify(
            1,
            10_000,
            ConnectorRetryMode::KnownNoEffect,
            ConnectorIdempotencyMode::Optional,
            &observation(
                ConnectorRetryClass::KnownNoEffect,
                false,
                EffectObservationState::NoEffect,
                false,
                None,
            ),
        )
        .expect("deadline deny");
    assert_eq!(
        deadline.deny_reason,
        Some(ConnectorRetryDenyReason::Deadline)
    );

    let mut tampered = observation(
        ConnectorRetryClass::KnownNoEffect,
        false,
        EffectObservationState::NoEffect,
        false,
        None,
    );
    tampered.code = "changed".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "connector_retry_observation_invalid"
    );
}

#[test]
fn unknown_fields_are_not_accepted() {
    let mut value = serde_json::to_value(observation(
        ConnectorRetryClass::KnownNoEffect,
        false,
        EffectObservationState::NoEffect,
        false,
        None,
    ))
    .expect("encode");
    value["raw_provider_error"] = json!("INT21_RAW");
    assert!(serde_json::from_value::<ConnectorRetryObservation>(value).is_err());
}
