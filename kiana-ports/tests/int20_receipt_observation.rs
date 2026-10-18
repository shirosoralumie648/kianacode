//! INT-20 Ports fixture: a prepared permit plus admitted owner/audience scope is the only source
//! of the receipt expectation.  The receipt and the observation never build their own binding, so
//! a receipt from another binding/account/operation/key, a late attempt, a swapped owner/audience
//! or a re-ordered evidence list is refused before any projection.

use kiana_domain::{
    json_digest, EffectObservation, EffectObservationState, ExecutionId, InvocationId,
    ProviderOutcome, ProviderReceipt, PROVIDER_RECEIPT_SCHEMA,
};
use kiana_ports::{
    ConnectorPreparedPermit, EffectObservationRequest, PortError,
    CONNECTOR_OBSERVATION_REQUEST_SCHEMA, CONNECTOR_PREPARED_PERMIT_SCHEMA,
};
use serde_json::json;

const NOW: u64 = 1_000;

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn payload_hash() -> String {
    kiana_domain::connector_payload_sha256(&json!({"status": "ok"}))
}

fn permit() -> ConnectorPreparedPermit {
    let mut permit = ConnectorPreparedPermit {
        schema: CONNECTOR_PREPARED_PERMIT_SCHEMA.to_owned(),
        connector_id: "connector-demo".to_owned(),
        connector_version: "v1".to_owned(),
        binding_id: "binding-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        operation: "create".to_owned(),
        invocation_id: InvocationId::new(),
        attempt: 1,
        action_digest: json_digest(&json!({"operation": "create"})),
        payload_digest: json_digest(&json!({"payload": "create"})),
        idempotency_key_digest: json_digest(&json!({"idempotency_key": "idem-1"})),
        authority_epoch: 1,
        policy_revision: "policy-1".to_owned(),
        binding_revision: 1,
        credential_generation: 1,
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + 500,
        permit_digest: String::new(),
    };
    permit.permit_digest = permit.digest();
    permit
}

fn receipt(permit: &ConnectorPreparedPermit, outcome: ProviderOutcome) -> ProviderReceipt {
    ProviderReceipt {
        schema: PROVIDER_RECEIPT_SCHEMA.to_owned(),
        connector_id: permit.connector_id.clone(),
        binding_id: permit.binding_id.clone(),
        account_id: permit.account_id.clone(),
        operation: permit.operation.clone(),
        idempotency_key: "idem-1".to_owned(),
        final_payload_sha256: payload_hash(),
        provider_receipt_id: "receipt-1".to_owned(),
        outcome,
        source: "local_fixture".to_owned(),
        result: json!({"status": "ok"}),
    }
}

fn request(permit: ConnectorPreparedPermit, receipt: ProviderReceipt) -> EffectObservationRequest {
    EffectObservationRequest {
        schema: CONNECTOR_OBSERVATION_REQUEST_SCHEMA.to_owned(),
        permit,
        receipt,
        owner_digest: digest("owner"),
        audience_digest: digest("audience"),
        observed_at_unix_ms: NOW + 1,
    }
}

#[test]
fn observation_request_derives_its_binding_from_the_permit_only() {
    let permit = permit();
    let request = request(permit.clone(), receipt(&permit, ProviderOutcome::Succeeded));
    request.validate().expect("observation request");

    let binding = request.binding().expect("binding");
    assert_eq!(binding.invocation_id, permit.invocation_id);
    assert_eq!(binding.attempt, permit.attempt);
    assert_eq!(
        binding.owner_digest.as_deref(),
        Some(digest("owner").as_str())
    );
    assert_eq!(
        binding.audience_digest.as_deref(),
        Some(digest("audience").as_str())
    );

    let mut foreign_binding = request.clone();
    foreign_binding.receipt.binding_id = "binding-other".to_owned();
    assert_eq!(
        foreign_binding.validate().unwrap_err(),
        PortError::Conflict("connector_observation_binding_mismatch".to_owned())
    );

    let mut foreign_account = request.clone();
    foreign_account.receipt.account_id = "account-other".to_owned();
    assert_eq!(
        foreign_account.validate().unwrap_err(),
        PortError::Conflict("connector_observation_binding_mismatch".to_owned())
    );

    let mut foreign_operation = request.clone();
    foreign_operation.receipt.operation = "read".to_owned();
    assert_eq!(
        foreign_operation.validate().unwrap_err(),
        PortError::Conflict("connector_observation_binding_mismatch".to_owned())
    );

    let mut foreign_key = request.clone();
    foreign_key.receipt.idempotency_key = "idem-2".to_owned();
    assert_eq!(
        foreign_key.validate().unwrap_err(),
        PortError::Conflict("connector_observation_binding_mismatch".to_owned())
    );

    let mut foreign_result = request.clone();
    foreign_result.receipt.result = json!({"access_token": "INT20_RAW_SECRET"});
    assert_eq!(
        foreign_result.validate().unwrap_err(),
        PortError::Failed(
            "connector_provider_receipt_invalid:provider_receipt_raw_response_forbidden".to_owned()
        )
    );

    let mut bad_scope = request;
    bad_scope.owner_digest = "not-a-digest".to_owned();
    assert_eq!(
        bad_scope.validate().unwrap_err(),
        PortError::Conflict("connector_observation_binding_mismatch".to_owned())
    );
}

#[test]
fn observation_projection_binds_attempt_scope_and_ordered_evidence() {
    let permit = permit();
    // Build the receipt from a borrow of the permit before `request` takes it by value.
    let receipt = receipt(&permit, ProviderOutcome::Succeeded);
    let request = request(permit, receipt);
    let binding = request.binding().expect("binding");
    let observation = EffectObservation::from_provider_receipt_for_binding(
        &request.receipt,
        &binding,
        ExecutionId::new(),
        NOW + 1,
        vec![
            digest("evidence-b"),
            digest("evidence-a"),
            digest("evidence-b"),
        ],
    )
    .expect("observation");
    assert_eq!(observation.state, EffectObservationState::ConfirmedSuccess);
    assert_eq!(
        observation.evidence_ref_digests,
        vec![digest("evidence-a"), digest("evidence-b")]
    );
    assert_eq!(
        observation.validate_for_receipt_binding(&request.receipt, &binding),
        Ok(())
    );

    let projection = observation.projection().expect("projection");
    let encoded = serde_json::to_string(&projection).expect("projection json");
    assert!(encoded.contains("result_digest"));
    assert!(!encoded.contains("\"result\":"));
    let decoded: kiana_domain::EffectObservationProjection =
        serde_json::from_str(&encoded).expect("projection decode");
    assert_eq!(decoded, projection);
    assert_eq!(decoded.validate(), Ok(()));

    let mut wrong_scope = binding.clone();
    wrong_scope.owner_digest = Some(digest("other-owner"));
    wrong_scope.binding_digest = wrong_scope.digest();
    assert_eq!(
        observation
            .validate_for_receipt_binding(&request.receipt, &wrong_scope)
            .unwrap_err(),
        "effect_observation_owner_or_audience_mismatch"
    );

    let late = EffectObservation::from_provider_receipt_for_binding(
        &request.receipt,
        &binding,
        ExecutionId::new(),
        NOW + 1,
        Vec::new(),
    )
    .expect("observation");
    let mut late_attempt = late;
    late_attempt.attempt = binding.attempt + 1;
    late_attempt.observation_digest = late_attempt.digest();
    assert_eq!(
        late_attempt
            .validate_for_receipt_binding(&request.receipt, &binding)
            .unwrap_err(),
        "effect_observation_attempt_mismatch"
    );

    let mut reordered = observation;
    reordered.evidence_ref_digests = vec![digest("evidence-b"), digest("evidence-a")];
    reordered.observation_digest = reordered.digest();
    assert_eq!(
        reordered.validate().unwrap_err(),
        "effect_observation_evidence_refs_unordered"
    );
}
