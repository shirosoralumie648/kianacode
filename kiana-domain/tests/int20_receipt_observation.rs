//! INT-20 deny-first fixtures for the provider receipt / effect observation contract.
//!
//! The suite proves, in order: foreign binding/account/operation/idempotency/payload evidence is
//! refused before projection; owner/audience/invocation/attempt come from the server-owned binding
//! and not from the observation itself; raw provider response material (strings, arrays, raw or
//! secret-shaped keys, oversized results) never becomes a receipt or a projection; evidence refs
//! are canonical and a re-ordered or duplicated list cannot be laundered into a different digest;
//! and succeeded/failed/unknown project to distinct, byte-stable digest-only views.

use kiana_domain::{
    connector_payload_sha256, json_digest, EffectObservation, EffectObservationProjection,
    EffectObservationState, ExecutionId, InvocationId, ProviderOutcome, ProviderReceipt,
    ProviderReceiptBinding, ProviderReceiptProjection, PROVIDER_RECEIPT_SCHEMA,
};
use serde_json::json;

const NOW: u64 = 1_700_000_000_000;

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn payload_hash() -> String {
    connector_payload_sha256(&json!({"status": "ok"}))
}

fn receipt(outcome: ProviderOutcome) -> ProviderReceipt {
    ProviderReceipt {
        schema: PROVIDER_RECEIPT_SCHEMA.to_owned(),
        connector_id: "connector-demo".to_owned(),
        binding_id: "binding-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        operation: "create".to_owned(),
        idempotency_key: "idem-1".to_owned(),
        final_payload_sha256: payload_hash(),
        provider_receipt_id: "receipt-1".to_owned(),
        outcome,
        source: "local_fixture".to_owned(),
        result: json!({"status": "ok"}),
    }
}

fn binding_for(
    receipt: &ProviderReceipt,
    payload_sha256: Option<String>,
) -> ProviderReceiptBinding {
    ProviderReceiptBinding::new(
        receipt.connector_id.clone(),
        receipt.binding_id.clone(),
        receipt.account_id.clone(),
        receipt.operation.clone(),
        InvocationId::new(),
        1,
        json_digest(&json!({"idempotency_key": receipt.idempotency_key})),
        payload_sha256,
        Some(digest("owner")),
        Some(digest("audience")),
    )
    .expect("binding")
}

fn observation_for(
    receipt: &ProviderReceipt,
    binding: &ProviderReceiptBinding,
) -> EffectObservation {
    EffectObservation::from_provider_receipt_for_binding(
        receipt,
        binding,
        ExecutionId::new(),
        NOW,
        vec![
            digest("evidence-b"),
            digest("evidence-a"),
            digest("evidence-b"),
        ],
    )
    .expect("observation")
}

#[test]
fn receipt_from_another_binding_account_operation_or_key_is_refused() {
    let receipt = receipt(ProviderOutcome::Succeeded);
    let binding = binding_for(&receipt, Some(payload_hash()));
    assert_eq!(receipt.validate_for_binding(&binding), Ok(()));

    let mut foreign_connector = receipt.clone();
    foreign_connector.connector_id = "connector-other".to_owned();
    let mut foreign_binding = receipt.clone();
    foreign_binding.binding_id = "binding-other".to_owned();
    let mut foreign_account = receipt.clone();
    foreign_account.account_id = "account-other".to_owned();
    let mut foreign_operation = receipt.clone();
    foreign_operation.operation = "read".to_owned();
    for (label, foreign) in [
        ("connector", foreign_connector),
        ("binding", foreign_binding),
        ("account", foreign_account),
        ("operation", foreign_operation),
    ] {
        assert_eq!(
            foreign.validate_for_binding(&binding).unwrap_err(),
            "provider_receipt_binding_mismatch",
            "{label} drift must be refused"
        );
    }

    let mut foreign_key = receipt.clone();
    foreign_key.idempotency_key = "idem-2".to_owned();
    assert_eq!(
        foreign_key.validate_for_binding(&binding).unwrap_err(),
        "provider_receipt_idempotency_mismatch"
    );

    let mut foreign_payload = receipt.clone();
    foreign_payload.final_payload_sha256 = "b".repeat(64);
    assert_eq!(
        foreign_payload.validate_for_binding(&binding).unwrap_err(),
        "provider_receipt_payload_mismatch"
    );

    // The historical unprefixed and the `sha256:` envelope spellings normalise to one canonical
    // payload hash and cannot be used to evade the binding.
    let mut prefixed = receipt.clone();
    prefixed.final_payload_sha256 = format!("sha256:{}", payload_hash());
    assert_eq!(prefixed.validate_for_binding(&binding), Ok(()));
    assert_eq!(prefixed.canonical_payload_sha256().unwrap(), payload_hash());

    // Identity-only expectations still refuse foreign identity but intentionally do not own a
    // payload hash (callers that have not seen the canonical payload cannot invent one).
    let identity_only = binding_for(&receipt, None);
    assert_eq!(foreign_payload.validate_for_binding(&identity_only), Ok(()));

    // A binding built through the constructor canonicalises the payload; a forged binding that
    // bypasses it fails its own header/digest self-proof.
    let mut forged = binding.clone();
    forged.payload_sha256 = Some("not-a-hash".to_owned());
    forged.binding_digest = forged.digest();
    assert_eq!(
        forged.validate().unwrap_err(),
        "provider_receipt_binding_header_invalid"
    );
}

#[test]
fn observation_scope_attempt_and_receipt_binding_fail_closed() {
    let receipt = receipt(ProviderOutcome::Succeeded);
    let binding = binding_for(&receipt, Some(payload_hash()));
    let observation = observation_for(&receipt, &binding);
    assert_eq!(
        observation.validate_for_receipt_binding(&receipt, &binding),
        Ok(())
    );

    assert_eq!(
        observation
            .validate_for_scope(&digest("other-owner"), &digest("audience"))
            .unwrap_err(),
        "effect_observation_owner_or_audience_mismatch"
    );
    assert_eq!(
        observation
            .validate_for_scope(&digest("owner"), &digest("other-audience"))
            .unwrap_err(),
        "effect_observation_owner_or_audience_mismatch"
    );

    let mut wrong_attempt = observation.clone();
    wrong_attempt.attempt = 2;
    wrong_attempt.observation_digest = wrong_attempt.digest();
    assert_eq!(
        wrong_attempt
            .validate_for_receipt_binding(&receipt, &binding)
            .unwrap_err(),
        "effect_observation_attempt_mismatch"
    );

    let mut wrong_invocation = observation.clone();
    wrong_invocation.invocation_id = InvocationId::new();
    wrong_invocation.observation_digest = wrong_invocation.digest();
    assert_eq!(
        wrong_invocation
            .validate_for_receipt_binding(&receipt, &binding)
            .unwrap_err(),
        "effect_observation_attempt_mismatch"
    );

    let mut foreign_receipt = receipt.clone();
    foreign_receipt.provider_receipt_id = "receipt-2".to_owned();
    assert_eq!(
        observation
            .validate_for_receipt_binding(&foreign_receipt, &binding)
            .unwrap_err(),
        "effect_observation_receipt_binding_mismatch"
    );

    let mut foreign_binding = binding.clone();
    foreign_binding.binding_id = "binding-other".to_owned();
    foreign_binding.binding_digest = foreign_binding.digest();
    assert_eq!(
        observation
            .validate_for_receipt_binding(&receipt, &foreign_binding)
            .unwrap_err(),
        "provider_receipt_binding_mismatch"
    );

    let mut swapped_key = observation.clone();
    swapped_key.idempotency_key_digest = digest("other-key");
    swapped_key.observation_digest = swapped_key.digest();
    assert_eq!(
        swapped_key
            .validate_for_receipt_binding(&receipt, &binding)
            .unwrap_err(),
        "effect_observation_receipt_binding_mismatch"
    );

    let mut missing_payload = observation.clone();
    missing_payload.payload_sha256 = None;
    missing_payload.observation_digest = missing_payload.digest();
    assert_eq!(
        missing_payload
            .validate_for_receipt_binding(&receipt, &binding)
            .unwrap_err(),
        "effect_observation_receipt_binding_mismatch"
    );

    // A scope-free expectation cannot validate an observation: scope must be explicit.
    let scope_free = ProviderReceiptBinding::new(
        receipt.connector_id.clone(),
        receipt.binding_id.clone(),
        receipt.account_id.clone(),
        receipt.operation.clone(),
        observation.invocation_id,
        observation.attempt,
        json_digest(&json!({"idempotency_key": receipt.idempotency_key})),
        Some(payload_hash()),
        None,
        None,
    )
    .expect("scope-free binding");
    assert_eq!(
        observation
            .validate_for_receipt_binding(&receipt, &scope_free)
            .unwrap_err(),
        "effect_observation_binding_scope_required"
    );
}

#[test]
fn succeeded_failed_and_unknown_project_distinct_stable_evidence() {
    for (outcome, state) in [
        (
            ProviderOutcome::Succeeded,
            EffectObservationState::ConfirmedSuccess,
        ),
        (
            ProviderOutcome::Failed,
            EffectObservationState::ConfirmedFailure,
        ),
        (ProviderOutcome::Unknown, EffectObservationState::Unknown),
    ] {
        let receipt = receipt(outcome);
        let binding = binding_for(&receipt, Some(payload_hash()));
        let observation = observation_for(&receipt, &binding);
        assert_eq!(
            observation.state, state,
            "{outcome:?} must keep its own state"
        );
        assert_eq!(
            observation.evidence_ref_digests,
            vec![digest("evidence-a"), digest("evidence-b")]
        );

        let projection = observation.projection().expect("projection");
        assert_eq!(projection.state, state);
        assert_eq!(
            projection.evidence_ref_digests,
            observation.evidence_ref_digests
        );
        assert_eq!(
            projection.observation_digest,
            observation.observation_digest
        );
        assert_eq!(projection.provider_receipt_id.as_deref(), Some("receipt-1"));

        // Re-projecting the same observation stays byte-stable and round-trips without widening.
        let repeated = observation.projection().expect("reprojection");
        assert_eq!(repeated.projection_digest, projection.projection_digest);
        let encoded = serde_json::to_string(&projection).expect("projection json");
        let decoded: EffectObservationProjection =
            serde_json::from_str(&encoded).expect("projection decode");
        assert_eq!(decoded, projection);
        assert_eq!(decoded.validate(), Ok(()));
        assert!(!encoded.contains("\"result\":"));
        assert!(!encoded.contains("raw_provider_response"));
        assert!(!encoded.contains("access_token"));

        // Evidence refs are canonicalised before the digest is taken, so caller array order can
        // never produce a second projection for the same evidence.
        let reordered = EffectObservation::from_provider_receipt_for_binding(
            &receipt,
            &binding,
            observation.execution_id,
            NOW,
            vec![digest("evidence-b"), digest("evidence-a")],
        )
        .expect("reordered observation");
        assert_eq!(reordered, observation);
        assert_eq!(
            reordered.projection().unwrap().projection_digest,
            projection.projection_digest
        );
    }
}

#[test]
fn unordered_duplicate_or_forged_evidence_refs_are_refused() {
    let receipt = receipt(ProviderOutcome::Succeeded);
    let binding = binding_for(&receipt, Some(payload_hash()));
    let observation = observation_for(&receipt, &binding);

    let mut reordered = observation.clone();
    reordered.evidence_ref_digests = vec![digest("evidence-b"), digest("evidence-a")];
    reordered.observation_digest = reordered.digest();
    assert_eq!(
        reordered.validate().unwrap_err(),
        "effect_observation_evidence_refs_unordered"
    );

    let mut duplicated = observation.clone();
    duplicated.evidence_ref_digests = vec![digest("evidence-a"), digest("evidence-a")];
    duplicated.observation_digest = duplicated.digest();
    assert_eq!(
        duplicated.validate().unwrap_err(),
        "effect_observation_evidence_refs_unordered"
    );

    let overflow = EffectObservation::from_provider_receipt_for_binding(
        &receipt,
        &binding,
        ExecutionId::new(),
        NOW,
        (0..129)
            .map(|index| digest(&format!("evidence-{index}")))
            .collect(),
    )
    .unwrap_err();
    assert_eq!(overflow, "effect_observation_header_invalid");

    let bad_digest = EffectObservation::from_provider_receipt_for_binding(
        &receipt,
        &binding,
        ExecutionId::new(),
        NOW,
        vec!["not-a-digest".to_owned()],
    )
    .unwrap_err();
    assert_eq!(bad_digest, "effect_observation_header_invalid");

    let projection = observation.projection().expect("projection");
    let mut forged_order: EffectObservationProjection =
        serde_json::from_value(serde_json::to_value(&projection).unwrap()).unwrap();
    forged_order.evidence_ref_digests = vec![digest("evidence-b"), digest("evidence-a")];
    forged_order.projection_digest = forged_order.digest();
    assert_eq!(
        forged_order.validate().unwrap_err(),
        "effect_observation_projection_invalid"
    );

    let mut tampered_digest = projection.clone();
    tampered_digest.projection_digest = digest("forged");
    assert_eq!(
        tampered_digest.validate().unwrap_err(),
        "effect_observation_projection_digest_mismatch"
    );

    let mut tampered_schema = projection;
    tampered_schema.schema = "kiana.effect-observation.v1".to_owned();
    assert_eq!(
        tampered_schema.validate().unwrap_err(),
        "effect_observation_projection_invalid"
    );
}

#[test]
fn raw_provider_response_shapes_never_leave_the_receipt() {
    let mut string_result = receipt(ProviderOutcome::Succeeded);
    string_result.result = json!("raw provider body");
    assert_eq!(
        string_result.validate().unwrap_err(),
        "provider_receipt_result_invalid"
    );
    assert_eq!(
        string_result.projection().unwrap_err(),
        "provider_receipt_result_invalid"
    );

    let mut array_result = receipt(ProviderOutcome::Succeeded);
    array_result.result = json!([{"ok": true}]);
    assert_eq!(
        array_result.validate().unwrap_err(),
        "provider_receipt_result_invalid"
    );

    let mut raw_key = receipt(ProviderOutcome::Succeeded);
    raw_key.result = json!({"raw_provider_response": {"body": "INT20_RAW_RESPONSE"}});
    assert_eq!(
        raw_key.validate().unwrap_err(),
        "provider_receipt_raw_response_forbidden"
    );

    let mut nested_key = receipt(ProviderOutcome::Succeeded);
    nested_key.result = json!({"wrapper": {"response_body": "INT20_RAW_RESPONSE"}});
    assert_eq!(
        nested_key.validate().unwrap_err(),
        "provider_receipt_raw_response_forbidden"
    );

    let mut secret_result = receipt(ProviderOutcome::Succeeded);
    secret_result.result = json!({"access_token": "INT20_RAW_SECRET"});
    assert_eq!(
        secret_result.validate().unwrap_err(),
        "provider_receipt_raw_response_forbidden"
    );
    let debug = format!("{secret_result:?}");
    assert!(!debug.contains("INT20_RAW_SECRET"));
    assert!(debug.contains("result_digest"));

    let mut secret_key = receipt(ProviderOutcome::Succeeded);
    secret_key.idempotency_key = "sk-live-INT20-SENTINEL".to_owned();
    assert_eq!(
        secret_key.validate().unwrap_err(),
        "provider_receipt_raw_response_forbidden"
    );

    let mut secret_source = receipt(ProviderOutcome::Succeeded);
    secret_source.source = "sk-live-INT20-SENTINEL".to_owned();
    assert_eq!(
        secret_source.validate().unwrap_err(),
        "provider_receipt_raw_response_forbidden"
    );

    let mut oversized = receipt(ProviderOutcome::Succeeded);
    oversized.result = json!({"body": "x".repeat(70 * 1024)});
    assert_eq!(
        oversized.validate().unwrap_err(),
        "provider_receipt_result_too_large"
    );

    let released = serde_json::to_string(
        &receipt(ProviderOutcome::Succeeded)
            .projection()
            .expect("projection"),
    )
    .expect("projection json");
    assert!(released.contains("result_digest"));
    assert!(!released.contains("\"result\""));
    assert!(!released.contains("INT20_RAW"));
}

#[test]
fn receipt_projection_is_canonical_and_key_order_independent() {
    let mut receipt = receipt(ProviderOutcome::Succeeded);
    receipt.result = json!({"a": 1, "b": 2});
    let projection = receipt.projection().expect("projection");

    let mut reordered = receipt.clone();
    reordered.result = json!({"b": 2, "a": 1});
    let reordered_projection = reordered.projection().expect("projection");
    assert_eq!(projection.result_digest, reordered_projection.result_digest);
    assert_eq!(
        projection.projection_digest,
        reordered_projection.projection_digest
    );

    let mut changed = receipt.clone();
    changed.result = json!({"a": 1, "b": 3});
    assert_ne!(
        changed.projection().unwrap().result_digest,
        projection.result_digest
    );

    let canonical_payload = receipt.final_payload_sha256.clone();
    let mut prefixed = receipt.clone();
    prefixed.final_payload_sha256 = format!("sha256:{canonical_payload}");
    let prefixed_projection = prefixed.projection().expect("projection");
    assert_eq!(prefixed_projection.payload_sha256, canonical_payload);
    assert_eq!(
        prefixed_projection.projection_digest,
        projection.projection_digest
    );

    let mut forged = projection.clone();
    forged.projection_digest = digest("forged");
    assert_eq!(
        forged.validate().unwrap_err(),
        "provider_receipt_projection_digest_mismatch"
    );

    let mut bad_schema = projection.clone();
    bad_schema.schema = "kiana.provider-receipt.v1".to_owned();
    assert_eq!(
        bad_schema.validate().unwrap_err(),
        "provider_receipt_projection_invalid"
    );

    let mut unknown_field = serde_json::to_value(&projection).unwrap();
    unknown_field["raw_provider_response"] = json!("INT20_RAW_RESPONSE");
    assert!(serde_json::from_value::<ProviderReceiptProjection>(unknown_field).is_err());
}

#[test]
fn receipt_binding_digest_covers_every_identity_and_scope_field() {
    let receipt = receipt(ProviderOutcome::Succeeded);
    let binding = binding_for(&receipt, Some(payload_hash()));
    let base_digest = binding.digest();

    let mutate = |binding: &ProviderReceiptBinding, apply: fn(&mut ProviderReceiptBinding)| {
        let mut mutated = binding.clone();
        apply(&mut mutated);
        mutated.binding_digest = mutated.digest();
        mutated
    };
    for mutated in [
        mutate(&binding, |value| {
            value.connector_id = "connector-other".to_owned()
        }),
        mutate(&binding, |value| {
            value.binding_id = "binding-other".to_owned()
        }),
        mutate(&binding, |value| {
            value.account_id = "account-other".to_owned()
        }),
        mutate(&binding, |value| value.operation = "read".to_owned()),
        mutate(&binding, |value| value.invocation_id = InvocationId::new()),
        mutate(&binding, |value| value.attempt = 2),
        mutate(&binding, |value| {
            value.idempotency_key_digest = digest("other-key")
        }),
        mutate(&binding, |value| {
            value.payload_sha256 = Some("c".repeat(64))
        }),
        mutate(&binding, |value| {
            value.owner_digest = Some(digest("other-owner"))
        }),
        mutate(&binding, |value| {
            value.audience_digest = Some(digest("other-audience"))
        }),
        mutate(&binding, |value| value.owner_digest = None),
    ] {
        assert_eq!(mutated.validate(), Ok(()));
        assert_ne!(mutated.digest(), base_digest);
    }

    let encoded = serde_json::to_string(&binding).expect("binding json");
    let decoded: ProviderReceiptBinding = serde_json::from_str(&encoded).expect("binding decode");
    assert_eq!(decoded, binding);
    assert_eq!(decoded.validate(), Ok(()));

    let mut forged = binding.clone();
    forged.binding_digest = digest("forged");
    assert_eq!(
        forged.validate().unwrap_err(),
        "provider_receipt_binding_digest_mismatch"
    );

    let mut unknown_field = serde_json::to_value(&binding).unwrap();
    unknown_field["raw_provider_response"] = json!("INT20_RAW_RESPONSE");
    assert!(serde_json::from_value::<ProviderReceiptBinding>(unknown_field).is_err());

    let nil = InvocationId::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    assert_eq!(
        ProviderReceiptBinding::new(
            "connector-demo",
            "binding-demo",
            "account-demo",
            "create",
            nil,
            1,
            digest("key"),
            Some(payload_hash()),
            Some(digest("owner")),
            Some(digest("audience")),
        )
        .unwrap_err(),
        "provider_receipt_binding_header_invalid"
    );
    assert_eq!(
        ProviderReceiptBinding::new(
            "connector-demo",
            "binding-demo",
            "account-demo",
            "create",
            InvocationId::new(),
            0,
            digest("key"),
            Some(payload_hash()),
            Some(digest("owner")),
            Some(digest("audience")),
        )
        .unwrap_err(),
        "provider_receipt_binding_header_invalid"
    );
    assert_eq!(
        ProviderReceiptBinding::new(
            "connector-demo",
            "binding-demo",
            "account-demo",
            "create",
            InvocationId::new(),
            1,
            digest("key"),
            Some("not-a-hash".to_owned()),
            Some(digest("owner")),
            Some(digest("audience")),
        )
        .unwrap_err(),
        "provider_receipt_binding_header_invalid"
    );

    // Constructor canonicalises a prefixed payload hash instead of storing the spelling.
    let prefixed_binding = ProviderReceiptBinding::new(
        "connector-demo",
        "binding-demo",
        "account-demo",
        "create",
        InvocationId::new(),
        1,
        digest("key"),
        Some(format!("sha256:{}", payload_hash())),
        Some(digest("owner")),
        Some(digest("audience")),
    )
    .expect("binding");
    assert_eq!(
        prefixed_binding.payload_sha256.as_deref(),
        Some(payload_hash().as_str())
    );
}

#[test]
fn receipt_headers_and_unknown_outcomes_fail_closed() {
    let mut schema_drift = receipt(ProviderOutcome::Succeeded);
    schema_drift.schema = "kiana.provider-receipt.v2".to_owned();
    assert_eq!(
        schema_drift.validate().unwrap_err(),
        "provider_receipt_header_invalid"
    );

    let mut empty_connector = receipt(ProviderOutcome::Succeeded);
    empty_connector.connector_id.clear();
    assert_eq!(
        empty_connector.validate().unwrap_err(),
        "provider_receipt_header_invalid"
    );

    let mut empty_key = receipt(ProviderOutcome::Succeeded);
    empty_key.idempotency_key.clear();
    assert_eq!(
        empty_key.validate().unwrap_err(),
        "provider_receipt_header_invalid"
    );

    let mut long_key = receipt(ProviderOutcome::Succeeded);
    long_key.idempotency_key = "k".repeat(129);
    assert_eq!(
        long_key.validate().unwrap_err(),
        "provider_receipt_header_invalid"
    );

    let mut control_key = receipt(ProviderOutcome::Succeeded);
    control_key.idempotency_key = "idem\n1".to_owned();
    assert_eq!(
        control_key.validate().unwrap_err(),
        "provider_receipt_header_invalid"
    );

    let mut bad_hash = receipt(ProviderOutcome::Succeeded);
    bad_hash.final_payload_sha256 = "not-a-hash".to_owned();
    assert_eq!(
        bad_hash.validate().unwrap_err(),
        "provider_receipt_header_invalid"
    );

    let mut bad_receipt_id = receipt(ProviderOutcome::Succeeded);
    bad_receipt_id.provider_receipt_id = "receipt 1".to_owned();
    assert_eq!(
        bad_receipt_id.validate().unwrap_err(),
        "provider_receipt_header_invalid"
    );

    let mut unknown_field = serde_json::to_value(receipt(ProviderOutcome::Succeeded)).unwrap();
    unknown_field["raw_provider_response"] = json!("INT20_RAW_RESPONSE");
    assert!(serde_json::from_value::<ProviderReceipt>(unknown_field).is_err());

    let mut unknown_outcome = serde_json::to_value(receipt(ProviderOutcome::Succeeded)).unwrap();
    unknown_outcome["outcome"] = json!("maybe");
    assert!(serde_json::from_value::<ProviderReceipt>(unknown_outcome).is_err());
}

#[test]
fn existing_receipt_observation_projection_contract_still_holds() {
    let receipt = receipt(ProviderOutcome::Succeeded);
    receipt.validate().expect("receipt");
    let binding = binding_for(&receipt, Some(payload_hash()));
    let observation = observation_for(&receipt, &binding);
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

    let encoded = observation.to_json().unwrap();
    assert_eq!(
        EffectObservation::from_json(&encoded).unwrap(),
        observation,
        "a valid observation must round-trip byte-stably"
    );
    let mut unknown = encoded.clone();
    unknown["raw_provider_response"] = json!("INT20_RAW_RESPONSE");
    assert_eq!(
        EffectObservation::from_json(&unknown).unwrap_err(),
        "effect_observation_decode_failed"
    );
}
