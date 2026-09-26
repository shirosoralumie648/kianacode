use kiana_domain::{
    json_digest, EffectObservation, EffectObservationState, ExecutionId, InvocationId,
    ProviderOutcome, ProviderReceipt, PROVIDER_RECEIPT_SCHEMA,
};
use serde_json::json;

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn receipt(outcome: ProviderOutcome) -> ProviderReceipt {
    ProviderReceipt {
        schema: PROVIDER_RECEIPT_SCHEMA.to_owned(),
        connector_id: "connector-demo".to_owned(),
        binding_id: "binding-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        operation: "create".to_owned(),
        idempotency_key: "idem-1".to_owned(),
        final_payload_sha256: "a".repeat(64),
        provider_receipt_id: "receipt-1".to_owned(),
        outcome,
        source: "local_fixture".to_owned(),
        result: json!({"status": "ok"}),
    }
}

fn observation(receipt: &ProviderReceipt) -> EffectObservation {
    EffectObservation::from_provider_receipt(
        receipt,
        ExecutionId::new(),
        InvocationId::new(),
        1,
        digest("owner"),
        digest("audience"),
        1_700_000_000_000,
    )
    .expect("observation")
}

#[test]
fn receipt_and_observation_bind_payload_owner_audience_and_outcome() {
    let receipt = receipt(ProviderOutcome::Succeeded);
    receipt.validate().expect("receipt");
    let observation = observation(&receipt);
    assert_eq!(
        observation.payload_sha256.as_deref(),
        Some(receipt.final_payload_sha256.as_str())
    );
    observation
        .validate_for_receipt(
            &receipt,
            &observation.owner_digest,
            &observation.audience_digest,
        )
        .expect("receipt binding");
    assert_eq!(observation.state, EffectObservationState::ConfirmedSuccess);

    let mut changed = receipt.clone();
    changed.final_payload_sha256 = "b".repeat(64);
    assert_eq!(
        observation
            .validate_for_receipt(
                &changed,
                &observation.owner_digest,
                &observation.audience_digest,
            )
            .unwrap_err(),
        "effect_observation_receipt_binding_mismatch"
    );
    assert_eq!(
        observation
            .validate_for_scope(&digest("other-owner"), &observation.audience_digest)
            .unwrap_err(),
        "effect_observation_owner_or_audience_mismatch"
    );
}

#[test]
fn failed_and_unknown_receipts_project_without_success_claims() {
    for (outcome, state) in [
        (
            ProviderOutcome::Failed,
            EffectObservationState::ConfirmedFailure,
        ),
        (ProviderOutcome::Unknown, EffectObservationState::Unknown),
    ] {
        let receipt = receipt(outcome);
        let observation = observation(&receipt);
        observation
            .validate_for_receipt(
                &receipt,
                &observation.owner_digest,
                &observation.audience_digest,
            )
            .expect("receipt binding");
        assert_eq!(observation.state, state);
    }
}

#[test]
fn raw_provider_response_and_unknown_fields_fail_closed() {
    let mut secret = receipt(ProviderOutcome::Succeeded);
    secret.result = json!({"access_token": "INT20_RAW_SECRET"});
    assert_eq!(
        secret.validate().unwrap_err(),
        "provider_receipt_raw_response_forbidden"
    );

    let mut encoded =
        serde_json::to_value(observation(&receipt(ProviderOutcome::Succeeded))).expect("encode");
    encoded["raw_provider_response"] = json!("INT20_RAW_RESPONSE");
    assert_eq!(
        EffectObservation::from_json(&encoded).unwrap_err(),
        "effect_observation_decode_failed"
    );

    let mut invalid_hash = receipt(ProviderOutcome::Succeeded);
    invalid_hash.final_payload_sha256 = "not-a-hash".to_owned();
    assert_eq!(
        invalid_hash.validate().unwrap_err(),
        "provider_receipt_header_invalid"
    );
}
