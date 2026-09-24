use async_trait::async_trait;
use kiana_domain::{
    effect_target_digest, AccountBinding, ConnectorBindingSnapshot, ConnectorDefinition,
    ConnectorEffect, ConnectorHttpsPolicy, ConnectorOperation, CredentialLease, InvocationId,
    ProviderOutcome, ProviderReceipt, SecretRef,
};
use kiana_ports::{
    CanonicalConnectorPayload, ConnectorHttpsRequest, ConnectorHttpsTransport,
    ConnectorHttpsTransportCapabilities, ConnectorPreparedPermit, PortError,
    CONNECTOR_HTTPS_REQUEST_SCHEMA, CONNECTOR_PREPARED_PERMIT_SCHEMA,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

const NOW: u64 = 10_000;

fn binding() -> ConnectorBindingSnapshot {
    let connector_id = "connector-https";
    let version = "v1";
    let definition = ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: connector_id.to_owned(),
        version: version.to_owned(),
        provider_id: "provider-https".to_owned(),
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
    let secret_ref = SecretRef::new(
        "env",
        "INT11_CONNECTOR_SECRET",
        kiana_domain::CONNECTOR_CREDENTIAL_PURPOSE,
        "connector:connector-https:v1",
        1,
    )
    .expect("secret reference");
    ConnectorBindingSnapshot {
        definition,
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: "binding-https".to_owned(),
            connector_id: connector_id.to_owned(),
            account_id: "account-https".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/connector.json".to_owned(),
            fixture_sha256: format!("sha256:{}", "a".repeat(64)),
            credential_ref: Some(secret_ref),
        },
        project_root: "/repo".to_owned(),
        revision: 1,
        status: "active".to_owned(),
    }
}

fn request() -> ConnectorHttpsRequest {
    let binding = binding();
    let payload = CanonicalConnectorPayload::new(json!({"query": "health"})).expect("payload");
    let mut permit = ConnectorPreparedPermit {
        schema: CONNECTOR_PREPARED_PERMIT_SCHEMA.to_owned(),
        connector_id: binding.definition.connector_id.clone(),
        connector_version: binding.definition.version.clone(),
        binding_id: binding.binding.binding_id.clone(),
        account_id: binding.binding.account_id.clone(),
        operation: "read".to_owned(),
        invocation_id: InvocationId::new(),
        attempt: 1,
        action_digest: kiana_domain::json_digest(&json!({"operation":"read"})),
        payload_digest: payload.payload_digest.clone(),
        idempotency_key_digest: kiana_domain::json_digest(&json!({"idempotency_key":"https-1"})),
        authority_epoch: 1,
        policy_revision: "policy-1".to_owned(),
        binding_revision: binding.revision,
        credential_generation: 1,
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + 500,
        permit_digest: String::new(),
    };
    permit.permit_digest = permit.digest();
    let lease = CredentialLease::issue(
        binding.binding.credential_ref.clone().expect("credential"),
        binding.binding.account_id.clone(),
        kiana_domain::CONNECTOR_CREDENTIAL_PURPOSE,
        "connector:connector-https:v1",
        effect_target_digest(&binding),
        NOW,
        400,
    )
    .expect("lease");
    let policy = ConnectorHttpsPolicy::from_parts(
        "https://api.example.com",
        vec!["api.example.com".to_owned()],
        vec!["93.184.216.34".to_owned()],
        Vec::new(),
        2,
    )
    .expect("https policy");
    let endpoint = policy
        .endpoint("https://api.example.com/v1")
        .expect("endpoint");
    let resolution = policy
        .observe_resolution(&endpoint, &["93.184.216.34".to_owned()])
        .expect("resolution");
    ConnectorHttpsRequest {
        schema: CONNECTOR_HTTPS_REQUEST_SCHEMA.to_owned(),
        permit,
        binding,
        lease,
        payload,
        policy,
        endpoint,
        resolution,
        proxy_origin: None,
        redirects: Vec::new(),
    }
}

fn receipt(request: &ConnectorHttpsRequest) -> ProviderReceipt {
    ProviderReceipt {
        schema: "kiana.provider-receipt.v1".to_owned(),
        connector_id: request.permit.connector_id.clone(),
        binding_id: request.permit.binding_id.clone(),
        account_id: request.permit.account_id.clone(),
        operation: request.permit.operation.clone(),
        idempotency_key: "https-1".to_owned(),
        final_payload_sha256: "b".repeat(64),
        provider_receipt_id: "provider-receipt-https".to_owned(),
        outcome: ProviderOutcome::Succeeded,
        source: "https_fixture".to_owned(),
        result: json!({"ok": true}),
    }
}

struct FakeHttpsTransport {
    sends: Arc<Mutex<u32>>,
}

#[async_trait]
impl ConnectorHttpsTransport for FakeHttpsTransport {
    fn capabilities(&self) -> ConnectorHttpsTransportCapabilities {
        ConnectorHttpsTransportCapabilities {
            tls_verified: true,
            dns_pinned: true,
            origin_pinned: true,
            explicit_proxy: false,
        }
    }

    async fn send(&self, request: ConnectorHttpsRequest) -> Result<ProviderReceipt, PortError> {
        *self.sends.lock().expect("send count") += 1;
        Ok(receipt(&request))
    }
}

#[tokio::test]
async fn fake_https_transport_only_dispatches_pinned_origin() {
    let sends = Arc::new(Mutex::new(0));
    let transport = FakeHttpsTransport {
        sends: sends.clone(),
    };
    let request = request();
    let result = transport.send_checked(request).await;
    assert!(result.is_ok());
    assert_eq!(*sends.lock().expect("send count"), 1);
}

#[tokio::test]
async fn proxy_or_cross_origin_redirect_is_rejected_before_fake_dispatch() {
    let sends = Arc::new(Mutex::new(0));
    let transport = FakeHttpsTransport {
        sends: sends.clone(),
    };
    let mut proxy_request = request();
    proxy_request.proxy_origin = Some("https://proxy.example.com".to_owned());
    let result = transport.send_checked(proxy_request).await;
    assert!(result.is_err());
    let mut redirect_request = request();
    let mut foreign_endpoint = redirect_request.endpoint.clone();
    foreign_endpoint.origin = "https://other.example.com:443".to_owned();
    foreign_endpoint.host = "other.example.com".to_owned();
    foreign_endpoint.endpoint_digest = foreign_endpoint.digest();
    redirect_request.redirects = vec![foreign_endpoint];
    let result = transport.send_checked(redirect_request).await;
    assert!(result.is_err());
    assert_eq!(*sends.lock().expect("send count"), 0);
}
