use kiana_domain::{
    EffectObservation, EffectObservationState, ExecutionId, InvocationId, ProviderOutcome,
    ProviderReceipt,
};
use serde_json::json;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn provider(outcome: ProviderOutcome) -> ProviderReceipt {
    ProviderReceipt {
        schema: "kiana.provider-receipt.v1".to_owned(),
        connector_id: "connector-demo".to_owned(),
        binding_id: "binding-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        operation: "create".to_owned(),
        idempotency_key: "idem-1".to_owned(),
        final_payload_sha256: "a".repeat(64),
        provider_receipt_id: "receipt-1".to_owned(),
        outcome,
        source: "local_fixture".to_owned(),
        result: json!({"status":"ok"}),
    }
}

#[test]
fn effect_observation_binds_provider_receipt_and_scope() {
    let owner = digest('a');
    let audience = digest('b');
    let observation = EffectObservation::from_provider_receipt(
        &provider(ProviderOutcome::Succeeded),
        ExecutionId::new(),
        InvocationId::new(),
        1,
        owner.clone(),
        audience.clone(),
        1_700_000_000_000,
    )
    .unwrap();
    assert_eq!(observation.state, EffectObservationState::ConfirmedSuccess);
    assert!(observation.validate_for_scope(&owner, &audience).is_ok());
    assert_eq!(
        EffectObservation::from_json(&observation.to_json().unwrap()).unwrap(),
        observation
    );
    assert_eq!(
        observation
            .validate_for_scope(&digest('c'), &audience)
            .unwrap_err(),
        "effect_observation_owner_or_audience_mismatch"
    );
}

#[test]
fn effect_observation_unknown_and_no_effect_never_claim_success() {
    let unknown = EffectObservation::from_provider_receipt(
        &provider(ProviderOutcome::Unknown),
        ExecutionId::new(),
        InvocationId::new(),
        1,
        digest('a'),
        digest('b'),
        1,
    )
    .unwrap();
    assert_eq!(unknown.state, EffectObservationState::Unknown);

    let no_effect = EffectObservation::new(
        ExecutionId::new(),
        InvocationId::new(),
        1,
        digest('a'),
        digest('b'),
        digest('c'),
        None,
        Some("not_started".to_owned()),
        1,
        Some(digest('d')),
        vec![digest('e')],
        EffectObservationState::NoEffect,
    )
    .unwrap();
    assert!(no_effect.validate().is_ok());

    let mut success_without_receipt = no_effect.clone();
    success_without_receipt.state = EffectObservationState::ConfirmedSuccess;
    success_without_receipt.observation_digest = success_without_receipt.digest();
    assert_eq!(
        success_without_receipt.validate().unwrap_err(),
        "effect_observation_provider_receipt_required"
    );
}

#[test]
fn effect_observation_rejects_unknown_fields_and_bad_provider_receipt() {
    let observation = EffectObservation::from_provider_receipt(
        &provider(ProviderOutcome::Failed),
        ExecutionId::new(),
        InvocationId::new(),
        1,
        digest('a'),
        digest('b'),
        1,
    )
    .unwrap();
    let mut unknown = observation.to_json().unwrap();
    unknown["raw_provider_result"] = json!("secret");
    assert_eq!(
        EffectObservation::from_json(&unknown).unwrap_err(),
        "effect_observation_decode_failed"
    );
    let mut bad = provider(ProviderOutcome::Succeeded);
    bad.idempotency_key.clear();
    assert_eq!(
        EffectObservation::from_provider_receipt(
            &bad,
            ExecutionId::new(),
            InvocationId::new(),
            1,
            digest('a'),
            digest('b'),
            1,
        )
        .unwrap_err(),
        "effect_observation_provider_receipt_invalid"
    );
}
