use async_trait::async_trait;
use kiana_domain::{
    effect_target_digest, json_digest, AccountBinding, ConnectorBindingSnapshot,
    ConnectorDefinition, ConnectorEffect, ConnectorOperation, EffectObservation, ExecutionId,
    ProcessGroupState, ProviderOutcome, ProviderReceipt, SecretRef, StopMethod, StopReport,
    WorkflowEventIngress, WorkflowEventSourcePolicy,
};
use kiana_ports::{
    CanonicalConnectorPayload, ConnectorAdapter, ConnectorAdapterCapabilities,
    ConnectorAdapterCapability, ConnectorCancelRequest, ConnectorHealth, ConnectorHealthStatus,
    ConnectorPreparedPermit, CredentialProbe, CredentialProbeRequest, CredentialProbeResult,
    EffectObservationRequest, EffectObserver, PortError, VerifiedWebhook,
    WebhookVerificationRequest, WebhookVerifier, CONNECTOR_CANCEL_REQUEST_SCHEMA,
    CONNECTOR_PORT_CONTRACT_SCHEMA, CONNECTOR_PROBE_RESULT_SCHEMA,
    CONNECTOR_WEBHOOK_REQUEST_SCHEMA,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const NOW: u64 = 1_000;

fn binding() -> ConnectorBindingSnapshot {
    let definition = ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: "connector-demo".to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-demo".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            "read".to_owned(),
            ConnectorOperation {
                effect: ConnectorEffect::ReadOnly,
                required_scope: "read".to_owned(),
                data_classes: BTreeSet::from(["internal".to_owned()]),
            },
        )]),
        rate_limit_per_minute: 10,
        idempotency_required: true,
        reconciliation_required: true,
        data_processing: "local_only".to_owned(),
    };
    let credential_ref = SecretRef::new(
        "env",
        "INT07_CONNECTOR_SECRET",
        kiana_domain::CONNECTOR_CREDENTIAL_PURPOSE,
        "connector:connector-demo:v1",
        4,
    )
    .unwrap();
    ConnectorBindingSnapshot {
        definition,
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: "binding-demo".to_owned(),
            connector_id: "connector-demo".to_owned(),
            account_id: "account-demo".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/connector.json".to_owned(),
            fixture_sha256: format!("sha256:{}", "a".repeat(64)),
            credential_ref: Some(credential_ref),
        },
        project_root: "/repo".to_owned(),
        revision: 7,
        status: "active".to_owned(),
    }
}

fn lease(binding: &ConnectorBindingSnapshot) -> kiana_domain::CredentialLease {
    kiana_domain::CredentialLease::issue(
        binding.binding.credential_ref.clone().unwrap(),
        binding.binding.account_id.clone(),
        kiana_domain::CONNECTOR_CREDENTIAL_PURPOSE,
        "connector:connector-demo:v1",
        effect_target_digest(binding),
        NOW,
        500,
    )
    .unwrap()
}

fn payload() -> CanonicalConnectorPayload {
    CanonicalConnectorPayload::new(json!({"query": "health"})).unwrap()
}

fn permit(
    binding: &ConnectorBindingSnapshot,
    payload: &CanonicalConnectorPayload,
    credential_generation: u64,
) -> ConnectorPreparedPermit {
    let mut permit = ConnectorPreparedPermit {
        schema: kiana_ports::CONNECTOR_PREPARED_PERMIT_SCHEMA.to_owned(),
        connector_id: binding.definition.connector_id.clone(),
        connector_version: binding.definition.version.clone(),
        binding_id: binding.binding.binding_id.clone(),
        account_id: binding.binding.account_id.clone(),
        operation: "read".to_owned(),
        invocation_id: kiana_domain::InvocationId::new(),
        attempt: 1,
        action_digest: json_digest(&json!({"operation": "read"})),
        payload_digest: payload.payload_digest.clone(),
        idempotency_key_digest: json_digest(&json!({"idempotency_key": "idempotency-1"})),
        authority_epoch: 1,
        policy_revision: "policy-1".to_owned(),
        binding_revision: binding.revision,
        credential_generation,
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + 500,
        permit_digest: String::new(),
    };
    permit.permit_digest = permit.digest();
    permit
}

fn receipt(binding: &ConnectorBindingSnapshot, outcome: ProviderOutcome) -> ProviderReceipt {
    ProviderReceipt {
        schema: "kiana.provider-receipt.v1".to_owned(),
        connector_id: binding.definition.connector_id.clone(),
        binding_id: binding.binding.binding_id.clone(),
        account_id: binding.binding.account_id.clone(),
        operation: "read".to_owned(),
        idempotency_key: "idempotency-1".to_owned(),
        final_payload_sha256: format!("sha256:{}", "b".repeat(64)),
        provider_receipt_id: "provider-receipt-1".to_owned(),
        outcome,
        source: "local_fixture".to_owned(),
        result: json!({"ok": true}),
    }
}

struct FakeAdapter {
    outcome: ProviderOutcome,
}

#[async_trait]
impl ConnectorAdapter for FakeAdapter {
    fn capabilities(&self) -> ConnectorAdapterCapabilities {
        ConnectorAdapterCapabilities::from_iter([
            ConnectorAdapterCapability::Invoke,
            ConnectorAdapterCapability::Cancel,
        ])
    }

    async fn invoke(
        &self,
        _permit: ConnectorPreparedPermit,
        _lease: kiana_domain::CredentialLease,
        _payload: CanonicalConnectorPayload,
    ) -> Result<ProviderReceipt, PortError> {
        Ok(receipt(&binding(), self.outcome))
    }

    async fn cancel(&self, request: ConnectorCancelRequest) -> Result<StopReport, PortError> {
        StopReport::new(
            request.permit.invocation_id.to_string(),
            None,
            None,
            StopMethod::None,
            false,
            false,
            true,
            ProcessGroupState::Empty,
            true,
            0,
        )
        .map_err(|error| PortError::Failed(error.to_owned()))
    }
}

struct FakeObserver;

#[async_trait]
impl EffectObserver for FakeObserver {
    fn supports_observation(&self) -> bool {
        true
    }

    async fn observe(
        &self,
        request: EffectObservationRequest,
    ) -> Result<EffectObservation, PortError> {
        EffectObservation::from_provider_receipt(
            &request.receipt,
            ExecutionId::new(),
            request.permit.invocation_id,
            request.permit.attempt,
            request.owner_digest,
            request.audience_digest,
            request.observed_at_unix_ms,
        )
        .map_err(PortError::Failed)
    }
}

struct FakeProbe;

#[async_trait]
impl CredentialProbe for FakeProbe {
    fn supports_probe(&self) -> bool {
        true
    }

    async fn probe(
        &self,
        request: CredentialProbeRequest,
    ) -> Result<CredentialProbeResult, PortError> {
        let health = ConnectorHealth::new(
            request.binding.definition.connector_id,
            request.binding.binding.binding_id,
            ConnectorHealthStatus::Verified,
            request.now_unix_ms,
            Some(json_digest(&json!({"probe": "verified"}))),
            vec!["fixture_only".to_owned()],
        )?;
        let result = CredentialProbeResult {
            schema: CONNECTOR_PROBE_RESULT_SCHEMA.to_owned(),
            status: ConnectorHealthStatus::Verified,
            health,
            credential_generation: request
                .lease
                .as_ref()
                .map(|lease| lease.secret_ref.generation),
            evidence_digest: Some(json_digest(&json!({"probe": "verified"}))),
        };
        result.validate()?;
        Ok(result)
    }
}

struct FakeWebhook;

#[async_trait]
impl WebhookVerifier for FakeWebhook {
    fn supports_webhook_verification(&self) -> bool {
        true
    }

    async fn verify(
        &self,
        request: WebhookVerificationRequest,
    ) -> Result<VerifiedWebhook, PortError> {
        let occurrence =
            kiana_domain::WorkflowEventOccurrence::from_verified(&request.ingress, &request.policy)
                .map_err(PortError::Failed)?;
        VerifiedWebhook::from_occurrence(occurrence, request.now_unix_ms)
    }
}

struct NoCapabilities;

#[tokio::test]
async fn fake_adapter_covers_known_unknown_and_stop_results() {
    let binding = binding();
    let payload = payload();
    let lease = lease(&binding);
    let permit = permit(&binding, &payload, lease.secret_ref.generation);
    for outcome in [ProviderOutcome::Succeeded, ProviderOutcome::Unknown] {
        let adapter = FakeAdapter { outcome };
        let value = adapter
            .invoke_checked(permit.clone(), lease.clone(), payload.clone(), &binding)
            .await
            .unwrap();
        assert_eq!(value.outcome, outcome);
    }

    let adapter = FakeAdapter {
        outcome: ProviderOutcome::Succeeded,
    };
    let report = adapter
        .cancel_checked(ConnectorCancelRequest {
            schema: CONNECTOR_CANCEL_REQUEST_SCHEMA.to_owned(),
            permit,
            reason: "fixture stop".to_owned(),
            requested_at_unix_ms: NOW,
        })
        .await
        .unwrap();
    assert!(report.confirmed);
}

#[tokio::test]
async fn probe_and_observer_return_redacted_health_and_effect_projection() {
    let binding = binding();
    let payload = payload();
    let lease = lease(&binding);
    let permit = permit(&binding, &payload, lease.secret_ref.generation);
    let probe = FakeProbe;
    let health = probe
        .probe_checked(CredentialProbeRequest {
            schema: CONNECTOR_PORT_CONTRACT_SCHEMA.to_owned(),
            binding: binding.clone(),
            operation: "read".to_owned(),
            lease: Some(lease),
            now_unix_ms: NOW + 1,
        })
        .await
        .unwrap();
    assert_eq!(health.status, ConnectorHealthStatus::Verified);
    let encoded = serde_json::to_string(&health).unwrap();
    assert!(!encoded.contains("INT07_RAW_SECRET_SENTINEL"));

    let observation = FakeObserver
        .observe_checked(EffectObservationRequest {
            schema: kiana_ports::CONNECTOR_OBSERVATION_REQUEST_SCHEMA.to_owned(),
            permit,
            receipt: receipt(&binding, ProviderOutcome::Unknown),
            owner_digest: json_digest(&json!({"owner": "project"})),
            audience_digest: json_digest(&json!({"binding": "binding-demo"})),
            observed_at_unix_ms: NOW + 1,
        })
        .await
        .unwrap();
    assert_eq!(
        observation.state,
        kiana_domain::EffectObservationState::Unknown
    );
}

#[tokio::test]
async fn webhook_verifier_returns_occurrence_without_payload_or_secret() {
    let ingress = WorkflowEventIngress::new(
        "source-demo",
        "key-demo",
        "project-demo",
        "trigger-demo",
        "event-demo",
        "health.changed",
        NOW,
        json!({"status": "ok"}),
        "signature-demo",
    )
    .unwrap();
    let policy = WorkflowEventSourcePolicy::new(
        "source-demo",
        "key-demo",
        "project-demo",
        ["health.changed".to_owned()],
        ["status".to_owned()],
        ["status".to_owned()],
        1_000,
    )
    .unwrap();
    let verified = FakeWebhook
        .verify_checked(WebhookVerificationRequest {
            schema: CONNECTOR_WEBHOOK_REQUEST_SCHEMA.to_owned(),
            ingress,
            policy,
            now_unix_ms: NOW + 1,
        })
        .await
        .unwrap();
    assert_eq!(verified.occurrence.event_id, "event-demo");
    assert!(!serde_json::to_string(&verified)
        .unwrap()
        .contains("signature-demo"));
}

#[tokio::test]
async fn missing_capabilities_fail_closed_before_adapter_calls() {
    let binding = binding();
    let payload = payload();
    let lease = lease(&binding);
    let permit = permit(&binding, &payload, lease.secret_ref.generation);
    let error = NoCapabilities
        .invoke_checked(permit, lease, payload, &binding)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PortError::Unavailable("connector_capability_missing:invoke".to_owned())
    );

    let descriptor = ConnectorAdapterCapabilities::default();
    assert!(!descriptor.supports(ConnectorAdapterCapability::Invoke));
}

#[async_trait]
impl ConnectorAdapter for NoCapabilities {}
