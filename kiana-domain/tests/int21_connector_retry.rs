//! INT-21 deny-first fixtures: the retry classifier is a pure, server-bound decision.
//!
//! Unknown effects, non-idempotent writes and approval/epoch/scope/validation/cancel failures are
//! explicit deny outcomes.  Only known-no-effect or provider-declared-idempotent evidence, bound
//! to the admitted invocation, retries — and only as a bounded next-attempt instruction that must
//! be re-admitted as a fresh INT-16 reservation/permit.  No fixture here dispatches, reserves or
//! writes an event.

use kiana_domain::{
    AccountBinding, ConnectorBindingSnapshot, ConnectorDefinition, ConnectorEffect,
    ConnectorIdempotencyMode, ConnectorInvocationCommand, ConnectorOperation,
    ConnectorRetryBinding, ConnectorRetryClass, ConnectorRetryDecision, ConnectorRetryDenyReason,
    ConnectorRetryMode, ConnectorRetryObservation, ConnectorRetryPolicy, EffectObservationState,
    InvocationId, MAX_CONNECTOR_RETRY_ATTEMPT_TIMEOUT_MS,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn snapshot() -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: ConnectorDefinition {
            schema: "kiana.connector-definition.v1".to_owned(),
            connector_id: "connector-int21".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-int21".to_owned(),
            adapter: "local_fixture".to_owned(),
            operations: BTreeMap::from([(
                "read".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            )]),
            rate_limit_per_minute: 10,
            idempotency_required: true,
            reconciliation_required: true,
            data_processing: "local_only".to_owned(),
        },
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: "binding-int21".to_owned(),
            connector_id: "connector-int21".to_owned(),
            account_id: "account-int21".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/int21.json".to_owned(),
            fixture_sha256: digest('f'),
            credential_ref: None,
        },
        project_root: "/tmp/int21".to_owned(),
        revision: 7,
        status: "active".to_owned(),
    }
}

fn command(attempt: u32) -> ConnectorInvocationCommand {
    ConnectorInvocationCommand::from_payload(
        InvocationId::new(),
        attempt,
        &snapshot(),
        "read",
        digest('o'),
        4,
        "policy.v7",
        "config.v1",
        0,
        9,
        &json!({"value": 1}),
        "retry-key",
    )
    .expect("invocation command")
}

/// The admitted invocation's idempotency key digest, as derived by the reservation command.
fn admitted_key_digest() -> String {
    command(1).idempotency_key_digest
}

fn binding(
    attempt: u32,
    retry_mode: ConnectorRetryMode,
    idempotency: ConnectorIdempotencyMode,
) -> ConnectorRetryBinding {
    ConnectorRetryBinding::from_command(&command(attempt), retry_mode, idempotency)
        .expect("retry binding")
}

fn policy() -> ConnectorRetryPolicy {
    ConnectorRetryPolicy::new(4, 2_000, 60_000, 5_000).expect("retry policy")
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
        declared_idempotent.then(admitted_key_digest),
        retry_after_ms,
        "connector_provider_error",
    )
    .expect("retry observation")
}

fn known_no_effect_evidence() -> ConnectorRetryObservation {
    observation(
        ConnectorRetryClass::KnownNoEffect,
        false,
        EffectObservationState::NoEffect,
        false,
        None,
    )
}

#[test]
fn unknown_non_idempotent_and_authority_classes_never_retry() {
    let policy = policy();
    let idempotent = binding(
        1,
        ConnectorRetryMode::DeclaredIdempotent,
        ConnectorIdempotencyMode::Optional,
    );

    // Deny classes: the classifier refuses to auto-retry an unknown effect or an
    // authorization/epoch/scope/validation/cancel failure even with a declared idempotency key.
    for (class, request_sent, state, reason) in [
        (
            ConnectorRetryClass::Unknown,
            true,
            EffectObservationState::Unknown,
            ConnectorRetryDenyReason::UnknownEffect,
        ),
        (
            ConnectorRetryClass::ApprovalDenied,
            false,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::ApprovalDenied,
        ),
        (
            ConnectorRetryClass::EpochStale,
            false,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::EpochStale,
        ),
        (
            ConnectorRetryClass::ScopeDenied,
            false,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::ScopeDenied,
        ),
        (
            ConnectorRetryClass::ValidationFailed,
            false,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::ValidationFailed,
        ),
        (
            ConnectorRetryClass::Cancelled,
            false,
            EffectObservationState::NoEffect,
            ConnectorRetryDenyReason::Cancelled,
        ),
    ] {
        let decision = policy
            .classify(
                &idempotent,
                1_000,
                &observation(class, request_sent, state, true, None),
            )
            .expect("deny decision");
        assert!(!decision.retry, "{class:?} must not retry");
        assert_eq!(decision.deny_reason, Some(reason), "{class:?}");
        assert_eq!(decision.next_attempt, 0);
        assert_eq!(decision.validate_for_binding(&idempotent), Ok(()));
    }

    // A timed-out request that may already have reached the provider is not known-no-effect.
    let timeout_after_send = policy
        .classify(
            &idempotent,
            1_000,
            &observation(
                ConnectorRetryClass::Timeout,
                true,
                EffectObservationState::Unknown,
                true,
                None,
            ),
        )
        .expect("timeout decision");
    assert_eq!(
        timeout_after_send.deny_reason,
        Some(ConnectorRetryDenyReason::UnknownEffect)
    );

    // Retry mode `never` denies even known-no-effect evidence.
    let never = binding(
        1,
        ConnectorRetryMode::Never,
        ConnectorIdempotencyMode::Optional,
    );
    let disabled = policy
        .classify(&never, 1_000, &known_no_effect_evidence())
        .expect("never decision");
    assert_eq!(
        disabled.deny_reason,
        Some(ConnectorRetryDenyReason::RetryModeDisabled)
    );

    // A forbidden idempotency policy never retries a request that was already sent.
    let forbidden = binding(
        1,
        ConnectorRetryMode::DeclaredIdempotent,
        ConnectorIdempotencyMode::Forbidden,
    );
    let non_idempotent = policy
        .classify(
            &forbidden,
            1_000,
            &observation(
                ConnectorRetryClass::ProviderFailed,
                true,
                EffectObservationState::NoEffect,
                false,
                None,
            ),
        )
        .expect("non-idempotent decision");
    assert_eq!(
        non_idempotent.deny_reason,
        Some(ConnectorRetryDenyReason::NonIdempotent)
    );

    // Evidence that claims an effect after a request was sent can never be retried.
    let sent_with_effect = policy
        .classify(
            &idempotent,
            1_000,
            &observation(
                ConnectorRetryClass::ProviderFailed,
                true,
                EffectObservationState::ConfirmedSuccess,
                true,
                None,
            ),
        )
        .expect("request already sent decision");
    assert_eq!(
        sent_with_effect.deny_reason,
        Some(ConnectorRetryDenyReason::RequestAlreadySent)
    );

    // Known-no-effect mode only accepts exactly known-no-effect evidence.
    let known_mode = binding(
        1,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let throttled = policy
        .classify(
            &known_mode,
            1_000,
            &observation(
                ConnectorRetryClass::ProviderThrottled,
                false,
                EffectObservationState::NoEffect,
                false,
                Some(50),
            ),
        )
        .expect("class not allowed decision");
    assert_eq!(
        throttled.deny_reason,
        Some(ConnectorRetryDenyReason::RetryClassNotAllowed)
    );
}

#[test]
fn attempt_budget_deadline_retry_after_and_the_timeout_window_fail_closed() {
    let policy = policy();
    let known = binding(
        1,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let idempotent = binding(
        1,
        ConnectorRetryMode::DeclaredIdempotent,
        ConnectorIdempotencyMode::Optional,
    );

    // The attempt budget is absolute: the last admitted attempt cannot start another one.
    let exhausted = binding(
        4,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let budget = policy
        .classify(&exhausted, 1_000, &known_no_effect_evidence())
        .expect("attempt budget decision");
    assert_eq!(
        budget.deny_reason,
        Some(ConnectorRetryDenyReason::AttemptBudget)
    );

    // The absolute deadline denies before any class-specific reason.
    let expired = policy
        .classify(&known, 60_000, &known_no_effect_evidence())
        .expect("deadline decision");
    assert_eq!(
        expired.deny_reason,
        Some(ConnectorRetryDenyReason::Deadline)
    );

    // Backoff is bounded by max_backoff_ms; a provider retry_after beyond it is refused.
    let too_late = policy
        .classify(
            &idempotent,
            1_000,
            &observation(
                ConnectorRetryClass::ProviderThrottled,
                true,
                EffectObservationState::NoEffect,
                true,
                Some(2_500),
            ),
        )
        .expect("retry_after decision");
    assert_eq!(
        too_late.deny_reason,
        Some(ConnectorRetryDenyReason::RetryAfterTooLarge)
    );

    // The new attempt must fit its own timeout window before the deadline: at 4_950 ms with a
    // 10_000 ms deadline, a 100 ms backoff plus a 5_000 ms attempt timeout is refused, while a
    // 1_000 ms attempt timeout still admits the retry.
    let tight_window = ConnectorRetryPolicy::new(4, 2_000, 10_000, 5_000).expect("tight policy");
    let window = tight_window
        .classify(&known, 4_950, &known_no_effect_evidence())
        .expect("timeout window decision");
    assert_eq!(window.deny_reason, Some(ConnectorRetryDenyReason::Deadline));

    let roomy_window = ConnectorRetryPolicy::new(4, 2_000, 10_000, 1_000).expect("roomy policy");
    let admitted = roomy_window
        .classify(&known, 4_950, &known_no_effect_evidence())
        .expect("admitted decision");
    assert!(admitted.retry);
    assert_eq!(admitted.next_attempt, 2);
    assert_eq!(admitted.delay_ms, 100);
}

#[test]
fn foreign_or_forged_retry_material_is_refused_before_a_decision() {
    let policy = policy();
    let known = binding(
        1,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let idempotent = binding(
        1,
        ConnectorRetryMode::DeclaredIdempotent,
        ConnectorIdempotencyMode::Optional,
    );

    // Evidence that references another invocation's idempotency key is refused, not retried.
    let foreign_key = ConnectorRetryObservation::new(
        ConnectorRetryClass::ProviderThrottled,
        true,
        EffectObservationState::NoEffect,
        true,
        Some(digest('x')),
        None,
        "connector_provider_error",
    )
    .expect("foreign key observation");
    assert_eq!(
        policy
            .classify(&idempotent, 1_000, &foreign_key)
            .unwrap_err(),
        "connector_retry_idempotency_key_mismatch"
    );

    // Forged observation shapes fail at construction.
    assert_eq!(
        ConnectorRetryObservation::new(
            ConnectorRetryClass::ProviderThrottled,
            true,
            EffectObservationState::NoEffect,
            true,
            None,
            None,
            "connector_provider_error",
        )
        .unwrap_err(),
        "connector_retry_idempotency_key_required"
    );
    assert_eq!(
        ConnectorRetryObservation::new(
            ConnectorRetryClass::ProviderFailed,
            false,
            EffectObservationState::ConfirmedSuccess,
            false,
            None,
            None,
            "connector_provider_error",
        )
        .unwrap_err(),
        "connector_retry_observation_effect_state_mismatch"
    );
    assert_eq!(
        ConnectorRetryObservation::new(
            ConnectorRetryClass::KnownNoEffect,
            false,
            EffectObservationState::NoEffect,
            false,
            None,
            Some(50),
            "connector_provider_error",
        )
        .unwrap_err(),
        "connector_retry_observation_retry_after_invalid"
    );
    assert_eq!(
        ConnectorRetryObservation::new(
            ConnectorRetryClass::Unknown,
            false,
            EffectObservationState::NoEffect,
            false,
            None,
            None,
            "connector_provider_error",
        )
        .unwrap_err(),
        "connector_retry_unknown_state_mismatch"
    );

    // A tampered observation no longer matches its own digest.
    let mut tampered = known_no_effect_evidence();
    tampered.code = "changed".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "connector_retry_observation_invalid"
    );
    assert_eq!(
        policy.classify(&known, 1_000, &tampered).unwrap_err(),
        "connector_retry_observation_invalid"
    );

    // Policy bounds are strict: zero/over-limit attempts, backoff and attempt timeout are refused.
    for invalid in [
        ConnectorRetryPolicy::new(0, 2_000, 60_000, 5_000),
        ConnectorRetryPolicy::new(17, 2_000, 60_000, 5_000),
        ConnectorRetryPolicy::new(4, 0, 60_000, 5_000),
        ConnectorRetryPolicy::new(4, 2_000, 60_000, 0),
        ConnectorRetryPolicy::new(4, 2_000, 60_000, MAX_CONNECTOR_RETRY_ATTEMPT_TIMEOUT_MS + 1),
        ConnectorRetryPolicy::new(4, 2_000, 0, 5_000),
    ] {
        assert_eq!(
            invalid.unwrap_err(),
            "connector_retry_policy_invalid",
            "invalid policy accepted"
        );
    }
    let mut tampered_policy = policy.clone();
    tampered_policy.deadline_unix_ms += 1;
    assert_eq!(
        tampered_policy.validate().unwrap_err(),
        "connector_retry_policy_invalid"
    );

    // Binding identity is server-owned: a tampered digest, a nil invocation, a zero attempt and a
    // contradictory retry/idempotency mode all fail closed before a decision exists.
    let mut tampered_binding = known.clone();
    tampered_binding.command_digest = digest('z');
    assert_eq!(
        policy
            .classify(&tampered_binding, 1_000, &known_no_effect_evidence())
            .unwrap_err(),
        "connector_retry_binding_invalid"
    );

    let mut nil_invocation = known.clone();
    nil_invocation.invocation_id = InvocationId::from_uuid(Uuid::nil());
    nil_invocation.binding_digest = nil_invocation.digest();
    assert_eq!(
        nil_invocation.validate().unwrap_err(),
        "connector_retry_binding_invalid"
    );

    let mut zero_attempt = known.clone();
    zero_attempt.attempt = 0;
    zero_attempt.binding_digest = zero_attempt.digest();
    assert_eq!(
        zero_attempt.validate().unwrap_err(),
        "connector_retry_binding_invalid"
    );

    assert_eq!(
        ConnectorRetryBinding::from_command(
            &command(1),
            ConnectorRetryMode::DeclaredIdempotent,
            ConnectorIdempotencyMode::Required,
        )
        .unwrap_err(),
        "connector_retry_binding_idempotency_conflict"
    );
    assert_eq!(
        ConnectorInvocationCommand::from_payload(
            InvocationId::new(),
            0,
            &snapshot(),
            "read",
            digest('o'),
            4,
            "policy.v7",
            "config.v1",
            0,
            9,
            &json!({"value": 1}),
            "retry-key",
        )
        .unwrap_err(),
        "connector_invocation_command_invalid"
    );

    // Unknown fields are rejected by every strict decoder.
    let mut observation_value = serde_json::to_value(known_no_effect_evidence()).expect("encode");
    observation_value["raw_provider_error"] = json!("INT21_RAW");
    assert!(serde_json::from_value::<ConnectorRetryObservation>(observation_value).is_err());

    let mut binding_value = serde_json::to_value(&known).expect("encode");
    binding_value["retry_authorized"] = json!(true);
    assert!(serde_json::from_value::<ConnectorRetryBinding>(binding_value).is_err());

    let mut policy_value = serde_json::to_value(&policy).expect("encode");
    policy_value["unlimited"] = json!(true);
    assert!(serde_json::from_value::<ConnectorRetryPolicy>(policy_value).is_err());

    let decision = policy
        .classify(&known, 1_000, &known_no_effect_evidence())
        .expect("decision");
    let mut decision_value = serde_json::to_value(&decision).expect("encode");
    decision_value["dispatch"] = json!(true);
    assert!(serde_json::from_value::<ConnectorRetryDecision>(decision_value).is_err());
}

#[test]
fn decisions_cannot_be_replayed_across_bindings_attempts_or_policies() {
    let policy = policy();
    let attempt_one = binding(
        1,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let attempt_two = binding(
        2,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let decision = policy
        .classify(&attempt_one, 1_000, &known_no_effect_evidence())
        .expect("decision");

    // A decision minted for attempt 1 cannot be applied to attempt 2's binding.
    assert_eq!(
        decision.validate_for_binding(&attempt_two).unwrap_err(),
        "connector_retry_decision_binding_mismatch"
    );
    assert_eq!(decision.validate_for_binding(&attempt_one), Ok(()));
    assert_eq!(decision.validate_for_policy(&policy), Ok(()));
    assert_eq!(decision.next_attempt, 2);

    // A forged decision that keeps attempt 1's binding digest but claims to be the next attempt
    // fails the attempt fence.
    let mut forged = decision.clone();
    forged.next_attempt = 1;
    forged.decision_digest = forged.digest();
    assert_eq!(
        forged.validate_for_binding(&attempt_one).unwrap_err(),
        "connector_retry_decision_attempt_mismatch"
    );

    // A deny decision is structurally empty, and a retry decision cannot claim zero delay.
    let mut denial = policy
        .classify(
            &binding(
                1,
                ConnectorRetryMode::Never,
                ConnectorIdempotencyMode::Optional,
            ),
            1_000,
            &known_no_effect_evidence(),
        )
        .expect("denial");
    denial.next_attempt = 1;
    denial.decision_digest = denial.digest();
    assert_eq!(
        denial.validate().unwrap_err(),
        "connector_retry_denial_invalid"
    );

    let mut zero_delay = decision.clone();
    zero_delay.delay_ms = 0;
    zero_delay.decision_digest = zero_delay.digest();
    assert_eq!(
        zero_delay.validate().unwrap_err(),
        "connector_retry_decision_invalid"
    );

    // The decision is bound to the exact policy that produced it.
    let other_policy = ConnectorRetryPolicy::new(4, 2_000, 60_001, 5_000).expect("other policy");
    assert_eq!(
        decision.validate_for_policy(&other_policy).unwrap_err(),
        "connector_retry_policy_digest_mismatch"
    );

    let mut tampered = decision.clone();
    tampered.delay_ms = 999;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "connector_retry_decision_invalid"
    );
}

#[test]
fn known_no_effect_and_declared_idempotent_retry_as_a_new_bound_attempt() {
    let policy = policy();

    // Known no effect: the request was never sent, so the bounded policy admits a new attempt.
    let known = binding(
        1,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let known_decision = policy
        .classify(&known, 1_000, &known_no_effect_evidence())
        .expect("known no effect decision");
    assert!(known_decision.retry);
    assert_eq!(known_decision.next_attempt, 2);
    assert_eq!(known_decision.delay_ms, 100);
    assert_eq!(known_decision.deny_reason, None);
    assert_eq!(known_decision.policy_digest, policy.policy_digest);
    assert_eq!(known_decision.binding_digest, known.binding_digest);
    assert_eq!(known_decision.validate_for_binding(&known), Ok(()));
    assert_eq!(known_decision.validate_for_policy(&policy), Ok(()));
    // Deterministic: the same inputs produce the same decision.
    assert_eq!(
        policy
            .classify(&known, 1_000, &known_no_effect_evidence())
            .expect("repeat decision"),
        known_decision
    );

    // Declared idempotent: a throttled request that was sent may retry, and the provider's
    // retry_after wins over the smaller exponential backoff step.
    let idempotent = binding(
        2,
        ConnectorRetryMode::DeclaredIdempotent,
        ConnectorIdempotencyMode::Optional,
    );
    let throttled = policy
        .classify(
            &idempotent,
            1_000,
            &observation(
                ConnectorRetryClass::ProviderThrottled,
                true,
                EffectObservationState::NoEffect,
                true,
                Some(700),
            ),
        )
        .expect("declared idempotent decision");
    assert!(throttled.retry);
    assert_eq!(throttled.next_attempt, 3);
    assert_eq!(throttled.delay_ms, 700);
    assert_eq!(throttled.validate_for_binding(&idempotent), Ok(()));

    // Backoff grows with the attempt and is clamped by max_backoff_ms.
    let third = binding(
        3,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let grown = policy
        .classify(&third, 1_000, &known_no_effect_evidence())
        .expect("grown decision");
    assert_eq!(grown.delay_ms, 400);

    let long_policy = ConnectorRetryPolicy::new(16, 2_000, 600_000, 5_000).expect("long policy");
    let eleventh = binding(
        11,
        ConnectorRetryMode::KnownNoEffect,
        ConnectorIdempotencyMode::Optional,
    );
    let clamped = long_policy
        .classify(&eleventh, 1_000, &known_no_effect_evidence())
        .expect("clamped decision");
    assert_eq!(clamped.delay_ms, 2_000);
    assert_eq!(clamped.next_attempt, 12);

    // The decision is a value contract only: it carries no reservation, permit or authorization
    // material that a caller could dispatch with.
    let encoded = serde_json::to_value(&known_decision).expect("encode");
    let keys: Vec<&str> = encoded
        .as_object()
        .expect("decision object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "schema",
            "retry",
            "next_attempt",
            "delay_ms",
            "deny_reason",
            "policy_digest",
            "binding_digest",
            "decision_digest",
        ]
    );
}
