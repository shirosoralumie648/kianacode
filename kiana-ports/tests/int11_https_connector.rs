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

fn capabilities_for(proxy: bool) -> ConnectorHttpsTransportCapabilities {
    ConnectorHttpsTransportCapabilities {
        tls_verified: true,
        dns_pinned: true,
        origin_pinned: true,
        explicit_proxy: proxy,
    }
}

fn denial_text(result: Result<ProviderReceipt, PortError>) -> String {
    result
        .expect_err("request must be denied before dispatch")
        .to_string()
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

/// Implements the port without overriding anything: both the capability default and the send
/// default must deny.
struct DefaultOnlyTransport;

impl ConnectorHttpsTransport for DefaultOnlyTransport {}

struct CapabilityTransport {
    sends: Arc<Mutex<u32>>,
    capabilities: ConnectorHttpsTransportCapabilities,
}

#[async_trait]
impl ConnectorHttpsTransport for CapabilityTransport {
    fn capabilities(&self) -> ConnectorHttpsTransportCapabilities {
        self.capabilities
    }

    async fn send(&self, request: ConnectorHttpsRequest) -> Result<ProviderReceipt, PortError> {
        *self.sends.lock().expect("send count") += 1;
        Ok(receipt(&request))
    }
}

struct ProxyCapableTransport {
    sends: Arc<Mutex<u32>>,
}

#[async_trait]
impl ConnectorHttpsTransport for ProxyCapableTransport {
    fn capabilities(&self) -> ConnectorHttpsTransportCapabilities {
        capabilities_for(true)
    }

    async fn send(&self, request: ConnectorHttpsRequest) -> Result<ProviderReceipt, PortError> {
        *self.sends.lock().expect("send count") += 1;
        Ok(receipt(&request))
    }
}

/// Well-behaved on the way out but returns a receipt for another operation, so the checked
/// wrapper has to reject it after dispatch.
struct ForeignReceiptTransport {
    sends: Arc<Mutex<u32>>,
}

#[async_trait]
impl ConnectorHttpsTransport for ForeignReceiptTransport {
    fn capabilities(&self) -> ConnectorHttpsTransportCapabilities {
        capabilities_for(false)
    }

    async fn send(&self, request: ConnectorHttpsRequest) -> Result<ProviderReceipt, PortError> {
        *self.sends.lock().expect("send count") += 1;
        let mut foreign = receipt(&request);
        foreign.operation = "write".to_owned();
        Ok(foreign)
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
async fn forged_credential_endpoint_binding_is_rejected_before_dispatch() {
    let sends = Arc::new(Mutex::new(0));
    let transport = FakeHttpsTransport {
        sends: sends.clone(),
    };
    let mut forged = request();
    forged.lease.endpoint_digest = kiana_domain::json_digest(&json!("foreign-target"));
    forged.lease.lease_digest = forged.lease.digest();

    assert!(transport.send_checked(forged).await.is_err());
    assert_eq!(*sends.lock().expect("send count"), 0);
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

#[tokio::test]
async fn default_capabilities_and_send_deny_without_any_dispatch() {
    let transport = DefaultOnlyTransport;
    let capabilities = transport.capabilities();
    assert_eq!(capabilities, ConnectorHttpsTransportCapabilities::default());
    assert!(!capabilities.tls_verified);
    assert!(!capabilities.dns_pinned);
    assert!(!capabilities.origin_pinned);
    assert!(!capabilities.explicit_proxy);

    let denial = denial_text(transport.send_checked(request()).await);
    assert_eq!(
        denial,
        "port_unavailable:connector_https_transport_boundary_unsupported"
    );
    // Even a direct call to the un-checked trait method cannot open a connection: the default
    // implementation is unsupported.
    let direct = transport
        .send(request())
        .await
        .expect_err("default send must deny");
    assert_eq!(
        direct.to_string(),
        "port_unavailable:connector_https_transport_unsupported"
    );
}

#[tokio::test]
async fn missing_security_capability_denies_before_the_implementation_runs() {
    let complete = capabilities_for(false);
    for capabilities in [
        ConnectorHttpsTransportCapabilities {
            tls_verified: false,
            ..complete
        },
        ConnectorHttpsTransportCapabilities {
            dns_pinned: false,
            ..complete
        },
        ConnectorHttpsTransportCapabilities {
            origin_pinned: false,
            ..complete
        },
        ConnectorHttpsTransportCapabilities::default(),
    ] {
        let sends = Arc::new(Mutex::new(0));
        let transport = CapabilityTransport {
            sends: sends.clone(),
            capabilities,
        };
        let denial = denial_text(transport.send_checked(request()).await);
        assert_eq!(
            denial,
            "port_unavailable:connector_https_transport_boundary_unsupported"
        );
        assert_eq!(*sends.lock().expect("send count"), 0);
    }
}

#[tokio::test]
async fn request_deny_matrix_never_reaches_the_transport() {
    let sends = Arc::new(Mutex::new(0));
    let transport = FakeHttpsTransport {
        sends: sends.clone(),
    };
    let mut cases: Vec<(String, ConnectorHttpsRequest, String)> = Vec::new();

    let mut schema = request();
    schema.schema = "kiana.connector-https-request.v2".to_owned();
    cases.push((
        "request schema".to_owned(),
        schema,
        "port_failed:connector_https_request_schema_invalid".to_owned(),
    ));

    let mut policy_digest = request();
    policy_digest.policy.policy_digest = kiana_domain::json_digest(&json!("forged-policy"));
    cases.push((
        "policy digest".to_owned(),
        policy_digest,
        "port_conflict:connector_https_policy_invalid".to_owned(),
    ));

    let mut payload_swap = request();
    payload_swap.payload =
        CanonicalConnectorPayload::new(json!({"query": "other"})).expect("other payload");
    cases.push((
        "payload swap".to_owned(),
        payload_swap,
        "port_conflict:connector_prepared_permit_binding_mismatch".to_owned(),
    ));

    let mut stale_binding = request();
    stale_binding.binding.revision += 1;
    cases.push((
        "stale binding revision".to_owned(),
        stale_binding,
        "port_conflict:connector_prepared_permit_binding_mismatch".to_owned(),
    ));

    let mut expired_lease = request();
    expired_lease.lease.expires_at_unix_ms = NOW;
    expired_lease.lease.lease_digest = expired_lease.lease.digest();
    cases.push((
        "expired lease".to_owned(),
        expired_lease,
        "port_conflict:connector_credential_lease:credential_lease_expired".to_owned(),
    ));

    let mut wrong_audience = request();
    wrong_audience.lease.audience = "connector:other-connector:v1".to_owned();
    wrong_audience.lease.lease_digest = wrong_audience.lease.digest();
    cases.push((
        "lease audience".to_owned(),
        wrong_audience,
        "port_conflict:connector_credential_lease:credential_lease_binding_mismatch".to_owned(),
    ));

    let mut endpoint_digest = request();
    endpoint_digest.endpoint.path = "/v2".to_owned();
    cases.push((
        "endpoint digest".to_owned(),
        endpoint_digest,
        "port_conflict:connector_https_origin_mismatch".to_owned(),
    ));

    let mut resolution_swap = request();
    resolution_swap.resolution.resolved_addresses = vec!["93.184.216.35".to_owned()];
    resolution_swap.resolution.resolution_digest = resolution_swap.resolution.digest();
    cases.push((
        "resolution swap".to_owned(),
        resolution_swap,
        "port_conflict:connector_https_resolution_invalid".to_owned(),
    ));

    let mut proxy = request();
    proxy.proxy_origin = Some("https://proxy.example.com".to_owned());
    cases.push((
        "proxy not allowlisted".to_owned(),
        proxy,
        "port_conflict:connector_https_proxy_denied".to_owned(),
    ));

    let mut cross_origin_redirect = request();
    let mut foreign_endpoint = cross_origin_redirect.endpoint.clone();
    foreign_endpoint.origin = "https://other.example.com:443".to_owned();
    foreign_endpoint.host = "other.example.com".to_owned();
    foreign_endpoint.endpoint_digest = foreign_endpoint.digest();
    cross_origin_redirect.redirects = vec![foreign_endpoint];
    cases.push((
        "cross origin redirect".to_owned(),
        cross_origin_redirect,
        "port_conflict:connector_https_origin_mismatch".to_owned(),
    ));

    let mut redirect_limit = request();
    let endpoint = redirect_limit.endpoint.clone();
    redirect_limit.redirects = vec![endpoint.clone(), endpoint.clone(), endpoint];
    cases.push((
        "redirect limit".to_owned(),
        redirect_limit,
        "port_conflict:connector_https_redirect_limit_exceeded".to_owned(),
    ));

    assert_eq!(cases.len(), 11);
    for (label, case, expected) in cases {
        let denial = denial_text(transport.send_checked(case).await);
        assert_eq!(denial, expected, "case {label}");
        assert!(
            !denial.contains("INT11_CONNECTOR_SECRET"),
            "case {label} leaked the secret reference key"
        );
        assert_eq!(*sends.lock().expect("send count"), 0, "case {label}");
    }
}

#[tokio::test]
async fn same_origin_redirect_chain_within_bound_dispatches_once() {
    let sends = Arc::new(Mutex::new(0));
    let transport = FakeHttpsTransport {
        sends: sends.clone(),
    };
    let mut request = request();
    let first = request
        .policy
        .redirect(&request.endpoint, "https://api.example.com/v2")
        .expect("first hop");
    let second = request
        .policy
        .redirect(&first, "https://api.example.com/v3")
        .expect("second hop");
    request.redirects = vec![first, second];

    let receipt = transport
        .send_checked(request)
        .await
        .expect("bounded same-origin redirect chain");
    assert_eq!(
        receipt.provider_receipt_id,
        "provider-receipt-https".to_owned()
    );
    assert_eq!(*sends.lock().expect("send count"), 1);
}

#[tokio::test]
async fn explicit_proxy_requires_an_allowlisted_origin_and_a_proxy_capable_transport() {
    let sends = Arc::new(Mutex::new(0));
    let mut request = request();
    request.policy = ConnectorHttpsPolicy::from_parts(
        "https://api.example.com",
        vec!["api.example.com".to_owned()],
        vec!["93.184.216.34".to_owned()],
        vec!["https://proxy.example.com".to_owned()],
        2,
    )
    .expect("proxy policy");
    let resolution = request
        .policy
        .observe_resolution(&request.endpoint, &["93.184.216.34".to_owned()])
        .expect("resolution");
    request.resolution = resolution;
    request.proxy_origin = Some("https://proxy.example.com".to_owned());

    // A transport that does not claim explicit proxy support must never silently pick a route.
    let plain = FakeHttpsTransport {
        sends: sends.clone(),
    };
    let denial = denial_text(plain.send_checked(request.clone()).await);
    assert_eq!(
        denial,
        "port_unavailable:connector_https_proxy_transport_unsupported"
    );
    assert_eq!(*sends.lock().expect("send count"), 0);

    // A proxy-capable transport still has to find the proxy on the policy allowlist; an ambient
    // or unlisted proxy origin is refused.
    let proxy_transport = ProxyCapableTransport {
        sends: sends.clone(),
    };
    let mut unlisted = request.clone();
    unlisted.proxy_origin = Some("https://ambient-proxy.example.com".to_owned());
    let denial = denial_text(proxy_transport.send_checked(unlisted).await);
    assert_eq!(denial, "port_conflict:connector_https_proxy_denied");
    assert_eq!(*sends.lock().expect("send count"), 0);

    let receipt = proxy_transport
        .send_checked(request)
        .await
        .expect("allowlisted proxy dispatch");
    assert_eq!(
        receipt.provider_receipt_id,
        "provider-receipt-https".to_owned()
    );
    assert_eq!(*sends.lock().expect("send count"), 1);
}

#[tokio::test]
async fn receipt_that_is_not_bound_to_the_permit_is_refused_after_dispatch() {
    let sends = Arc::new(Mutex::new(0));
    let transport = ForeignReceiptTransport {
        sends: sends.clone(),
    };
    let denial = denial_text(transport.send_checked(request()).await);
    assert_eq!(
        denial,
        "port_conflict:connector_provider_receipt_binding_invalid"
    );
    assert_eq!(*sends.lock().expect("send count"), 1);
}

#[test]
fn request_contract_rejects_unknown_fields_and_absent_proxy_is_not_serialized() {
    let request = request();
    let encoded = serde_json::to_value(&request).expect("encode request");
    // The request has no ambient-proxy field at all; an absent explicit proxy is not serialized
    // and an empty redirect list stays an empty list.
    assert!(encoded.get("proxy_origin").is_none());
    assert_eq!(encoded.get("redirects"), Some(&json!([])));

    let mut unknown = encoded.clone();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorHttpsRequest>(unknown).is_err());

    let mut explicit_proxy = encoded;
    explicit_proxy["proxy_origin"] = json!("https://proxy.example.com");
    let decoded: ConnectorHttpsRequest =
        serde_json::from_value(explicit_proxy).expect("explicit proxy decodes");
    assert_eq!(
        decoded.proxy_origin.as_deref(),
        Some("https://proxy.example.com")
    );
    assert_eq!(decoded.schema, CONNECTOR_HTTPS_REQUEST_SCHEMA);

    let capabilities = ConnectorHttpsTransportCapabilities::default();
    let encoded_capabilities = serde_json::to_value(capabilities).expect("encode capabilities");
    let decoded_capabilities: ConnectorHttpsTransportCapabilities =
        serde_json::from_value(encoded_capabilities.clone()).expect("decode capabilities");
    assert_eq!(decoded_capabilities, capabilities);
    let mut unknown_capabilities = encoded_capabilities;
    unknown_capabilities["ambient_proxy"] = json!(true);
    assert!(
        serde_json::from_value::<ConnectorHttpsTransportCapabilities>(unknown_capabilities)
            .is_err()
    );
}
