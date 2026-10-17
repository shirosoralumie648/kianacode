//! INT-07 connector Port 的合成适配器测试。
//!
//! 本文件以 Fake 实现检查 checked wrapper 的输入校验、结果合同和敏感字段投影，不连接真实服务，也不从
//! EventStore/ControlPlane 取得授权事实。尤其 `FakeWebhook` 只演示 occurrence 如何被包装和脱敏；它没有
//! HMAC key，不代表生产 Webhook 签名认证已经接入。
//!
//! 2026-10-17 completion slice 增加四条先拒绝夹具：`Sealed*` fake 记录是否被调用，证明缺能力时八个
//! checked 入口都在实现之前 fail-closed；`PermissiveAdapter` 声明全部能力，证明能力登记不能替代
//! prepared permit/租约绑定；`ReceiptAdapter`/`ForeignReceiptObserver`/`LateAttemptObserver` 证明与
//! 许可对不上的回执、别的回执派生的观测和下一 attempt 的观测都被拒；投影类型（probe result、effect
//! observation、verified webhook、health）拒绝未知字段、raw secret 只能以 digest 形状出现，回执里回显
//! raw secret 会被域校验拒绝且错误投影不回显哨兵。

use async_trait::async_trait;
use kiana_domain::{
    effect_target_digest, json_digest, AccountBinding, ConnectorBindingSnapshot,
    ConnectorDefinition, ConnectorEffect, ConnectorOperation, EffectObservation, ExecutionId,
    McpCapabilityHandshake, ProcessGroupState, ProviderOutcome, ProviderReceipt, SecretRef,
    StopMethod, StopReport, WorkflowEventIngress, WorkflowEventSourcePolicy,
};
use kiana_ports::{
    CanonicalConnectorPayload, ConnectorAdapter, ConnectorAdapterCapabilities,
    ConnectorAdapterCapability, ConnectorAdapterDescriptor, ConnectorCancelRequest,
    ConnectorHealth, ConnectorHealthStatus, ConnectorPreparedPermit, CredentialProbe,
    CredentialProbeRequest, CredentialProbeResult, EffectObservationRequest, EffectObserver,
    McpCapabilityHandshakeRequest, PortError, VerifiedWebhook, WebhookVerificationRequest,
    WebhookVerifier, CONNECTOR_CANCEL_REQUEST_SCHEMA, CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA,
    CONNECTOR_OBSERVATION_REQUEST_SCHEMA, CONNECTOR_PORT_CONTRACT_SCHEMA,
    CONNECTOR_PROBE_RESULT_SCHEMA, CONNECTOR_WEBHOOK_REQUEST_SCHEMA,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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

fn owner_digest() -> String {
    json_digest(&json!({"owner": "project"}))
}

fn audience_digest() -> String {
    json_digest(&json!({"binding": "binding-demo"}))
}

fn ingress() -> WorkflowEventIngress {
    WorkflowEventIngress::new(
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
    .unwrap()
}

fn policy() -> WorkflowEventSourcePolicy {
    WorkflowEventSourcePolicy::new(
        "source-demo",
        "key-demo",
        "project-demo",
        ["health.changed".to_owned()],
        ["status".to_owned()],
        ["status".to_owned()],
        1_000,
    )
    .unwrap()
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

/// 只用于验证 Port DTO 包装形状的假 Webhook adapter，不执行密码学签名验证。
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
        // 此 fixture 直接调用 Domain 的调用者前置 API；签名文本甚至只是普通占位字符串。
        // 因而这个测试只能证明 occurrence 可包装且结果不回显原签名，不能证明 HMAC 已通过。
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
    // 该输入在 Port 层通过结构校验即可进入 FakeWebhook；真正的 Daemon verifier 有独立的 HMAC 测试。
    let verified = FakeWebhook
        .verify_checked(WebhookVerificationRequest {
            schema: CONNECTOR_WEBHOOK_REQUEST_SCHEMA.to_owned(),
            ingress: ingress(),
            policy: policy(),
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

/// 记录是否被调用的假适配器：它不声明任何能力，因此任何一个 checked 入口都必须在调用实现之前拒绝。
struct SealedAdapter {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ConnectorAdapter for SealedAdapter {
    async fn discover(
        &self,
        _binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorAdapterDescriptor, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("sealed_discover_called".to_owned()))
    }

    async fn validate_binding(
        &self,
        _binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorHealth, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("sealed_health_called".to_owned()))
    }

    async fn invoke(
        &self,
        _permit: ConnectorPreparedPermit,
        _lease: kiana_domain::CredentialLease,
        _payload: CanonicalConnectorPayload,
    ) -> Result<ProviderReceipt, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("sealed_invoke_called".to_owned()))
    }

    async fn cancel(&self, _request: ConnectorCancelRequest) -> Result<StopReport, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("sealed_cancel_called".to_owned()))
    }

    async fn capability_handshake(
        &self,
        _request: McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("sealed_handshake_called".to_owned()))
    }
}

struct SealedObserver {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl EffectObserver for SealedObserver {
    async fn observe(
        &self,
        _request: EffectObservationRequest,
    ) -> Result<EffectObservation, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("sealed_observe_called".to_owned()))
    }
}

struct SealedProbe {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl CredentialProbe for SealedProbe {
    async fn probe(
        &self,
        _request: CredentialProbeRequest,
    ) -> Result<CredentialProbeResult, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("sealed_probe_called".to_owned()))
    }
}

struct SealedWebhook {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl WebhookVerifier for SealedWebhook {
    async fn verify(
        &self,
        _request: WebhookVerificationRequest,
    ) -> Result<VerifiedWebhook, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("sealed_verify_called".to_owned()))
    }
}

/// 声明全部能力（含 observe_receipt）的假适配器。能力只是描述，不能替代服务端持有的许可/租约。
struct PermissiveAdapter {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ConnectorAdapter for PermissiveAdapter {
    fn capabilities(&self) -> ConnectorAdapterCapabilities {
        ConnectorAdapterCapabilities::from_iter([
            ConnectorAdapterCapability::Discover,
            ConnectorAdapterCapability::ReadOnlyProbe,
            ConnectorAdapterCapability::Invoke,
            ConnectorAdapterCapability::ObserveReceipt,
            ConnectorAdapterCapability::Cancel,
            ConnectorAdapterCapability::WebhookVerify,
            ConnectorAdapterCapability::CapabilityHandshake,
        ])
    }

    async fn validate_binding(
        &self,
        binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorHealth, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        ConnectorHealth::new(
            binding.definition.connector_id.clone(),
            binding.binding.binding_id.clone(),
            ConnectorHealthStatus::Verified,
            NOW + 1,
            None,
            Vec::new(),
        )
    }

    async fn invoke(
        &self,
        _permit: ConnectorPreparedPermit,
        _lease: kiana_domain::CredentialLease,
        _payload: CanonicalConnectorPayload,
    ) -> Result<ProviderReceipt, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(receipt(&binding(), ProviderOutcome::Succeeded))
    }
}

#[async_trait]
impl CredentialProbe for PermissiveAdapter {
    fn supports_probe(&self) -> bool {
        true
    }

    async fn probe(
        &self,
        request: CredentialProbeRequest,
    ) -> Result<CredentialProbeResult, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let health = ConnectorHealth::new(
            request.binding.definition.connector_id.clone(),
            request.binding.binding.binding_id.clone(),
            ConnectorHealthStatus::ConnectivityOnly,
            request.now_unix_ms,
            None,
            Vec::new(),
        )?;
        let result = CredentialProbeResult {
            schema: CONNECTOR_PROBE_RESULT_SCHEMA.to_owned(),
            status: ConnectorHealthStatus::ConnectivityOnly,
            health,
            credential_generation: request
                .lease
                .as_ref()
                .map(|lease| lease.secret_ref.generation),
            evidence_digest: None,
        };
        result.validate()?;
        Ok(result)
    }
}

/// 返回调用方提供回执的假适配器，用于检查 checked 包装器是否把「回执 ↔ 许可」绑定钉住。
struct ReceiptAdapter {
    receipt: ProviderReceipt,
}

#[async_trait]
impl ConnectorAdapter for ReceiptAdapter {
    fn capabilities(&self) -> ConnectorAdapterCapabilities {
        ConnectorAdapterCapabilities::from_iter([ConnectorAdapterCapability::Invoke])
    }

    async fn invoke(
        &self,
        _permit: ConnectorPreparedPermit,
        _lease: kiana_domain::CredentialLease,
        _payload: CanonicalConnectorPayload,
    ) -> Result<ProviderReceipt, PortError> {
        Ok(self.receipt.clone())
    }
}

/// 返回「另一份回执」观测的假 observer：payload/idempotency 相同但 provider_receipt_id 不同。
struct ForeignReceiptObserver {
    foreign: ProviderReceipt,
}

#[async_trait]
impl EffectObserver for ForeignReceiptObserver {
    fn supports_observation(&self) -> bool {
        true
    }

    async fn observe(
        &self,
        request: EffectObservationRequest,
    ) -> Result<EffectObservation, PortError> {
        EffectObservation::from_provider_receipt(
            &self.foreign,
            ExecutionId::new(),
            request.permit.invocation_id,
            request.permit.attempt,
            request.owner_digest.clone(),
            request.audience_digest.clone(),
            request.observed_at_unix_ms,
        )
        .map_err(PortError::Failed)
    }
}

/// 返回「下一次 attempt」观测的假 observer：回执本身匹配，只有 attempt 不一致。
struct LateAttemptObserver;

#[async_trait]
impl EffectObserver for LateAttemptObserver {
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
            request.permit.attempt + 1,
            request.owner_digest.clone(),
            request.audience_digest.clone(),
            request.observed_at_unix_ms,
        )
        .map_err(PortError::Failed)
    }
}

#[tokio::test]
async fn every_checked_port_entry_point_denies_a_missing_capability_before_calling_the_implementation(
) {
    let binding = binding();
    let payload = payload();
    let lease = lease(&binding);
    let permit = permit(&binding, &payload, lease.secret_ref.generation);

    let adapter = SealedAdapter {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let observer = SealedObserver {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let probe = SealedProbe {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let webhook = SealedWebhook {
        calls: Arc::new(AtomicUsize::new(0)),
    };

    assert_eq!(
        adapter.discover_checked(&binding).await.unwrap_err(),
        PortError::Unavailable("connector_capability_missing:discover".to_owned())
    );
    assert_eq!(
        adapter
            .validate_binding_checked(&binding)
            .await
            .unwrap_err(),
        PortError::Unavailable("connector_capability_missing:read_only_probe".to_owned())
    );
    assert_eq!(
        adapter
            .invoke_checked(permit.clone(), lease.clone(), payload.clone(), &binding)
            .await
            .unwrap_err(),
        PortError::Unavailable("connector_capability_missing:invoke".to_owned())
    );
    assert_eq!(
        adapter
            .cancel_checked(ConnectorCancelRequest {
                schema: CONNECTOR_CANCEL_REQUEST_SCHEMA.to_owned(),
                permit: permit.clone(),
                reason: "fixture stop".to_owned(),
                requested_at_unix_ms: NOW,
            })
            .await
            .unwrap_err(),
        PortError::Unavailable("connector_capability_missing:cancel".to_owned())
    );
    assert_eq!(
        adapter
            .capability_handshake_checked(McpCapabilityHandshakeRequest {
                schema: CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA.to_owned(),
                binding: binding.clone(),
                server: "server-demo".to_owned(),
                session_ref: "session-demo".to_owned(),
            })
            .await
            .unwrap_err(),
        PortError::Unavailable("connector_capability_missing:capability_handshake".to_owned())
    );
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);

    assert_eq!(
        observer
            .observe_checked(EffectObservationRequest {
                schema: CONNECTOR_OBSERVATION_REQUEST_SCHEMA.to_owned(),
                permit: permit.clone(),
                receipt: receipt(&binding, ProviderOutcome::Succeeded),
                owner_digest: owner_digest(),
                audience_digest: audience_digest(),
                observed_at_unix_ms: NOW + 1,
            })
            .await
            .unwrap_err(),
        PortError::Unavailable("connector_capability_missing:observe_receipt".to_owned())
    );
    assert_eq!(
        probe
            .probe_checked(CredentialProbeRequest {
                schema: CONNECTOR_PORT_CONTRACT_SCHEMA.to_owned(),
                binding: binding.clone(),
                operation: "read".to_owned(),
                lease: Some(lease.clone()),
                now_unix_ms: NOW + 1,
            })
            .await
            .unwrap_err(),
        PortError::Unavailable("connector_capability_missing:read_only_probe".to_owned())
    );
    assert_eq!(
        webhook
            .verify_checked(WebhookVerificationRequest {
                schema: CONNECTOR_WEBHOOK_REQUEST_SCHEMA.to_owned(),
                ingress: ingress(),
                policy: policy(),
                now_unix_ms: NOW + 1,
            })
            .await
            .unwrap_err(),
        PortError::Unavailable("connector_capability_missing:webhook_verify".to_owned())
    );

    assert_eq!(observer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    assert_eq!(webhook.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn registered_capabilities_are_not_authority_over_the_prepared_permit() {
    let binding = binding();
    let payload = payload();
    let lease = lease(&binding);
    let adapter = PermissiveAdapter {
        calls: Arc::new(AtomicUsize::new(0)),
    };

    // 能力登记是描述：七项全部声明，缺能力以外的判断仍全部发生在包装器内。
    let capabilities = adapter.capabilities();
    for capability in [
        ConnectorAdapterCapability::Discover,
        ConnectorAdapterCapability::ReadOnlyProbe,
        ConnectorAdapterCapability::Invoke,
        ConnectorAdapterCapability::ObserveReceipt,
        ConnectorAdapterCapability::Cancel,
        ConnectorAdapterCapability::WebhookVerify,
        ConnectorAdapterCapability::CapabilityHandshake,
    ] {
        assert!(capabilities.supports(capability));
    }

    // 许可绑定的是旧 binding revision：能力齐全也到不了实现。
    let mut stale = permit(&binding, &payload, lease.secret_ref.generation);
    stale.binding_revision = binding.revision + 1;
    stale.permit_digest = stale.digest();
    assert_eq!(
        adapter
            .invoke_checked(stale, lease.clone(), payload.clone(), &binding)
            .await
            .unwrap_err(),
        PortError::Conflict("connector_prepared_permit_binding_mismatch".to_owned())
    );
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);

    // 探测租约属于另一个 SecretRef：即使声明了 read_only_probe 也被拒。
    let foreign_secret = SecretRef::new(
        "env",
        "INT07_FOREIGN_CONNECTOR_SECRET",
        kiana_domain::CONNECTOR_CREDENTIAL_PURPOSE,
        "connector:connector-demo:v1",
        9,
    )
    .unwrap();
    let foreign_lease = kiana_domain::CredentialLease::issue(
        foreign_secret,
        binding.binding.account_id.clone(),
        kiana_domain::CONNECTOR_CREDENTIAL_PURPOSE,
        "connector:connector-demo:v1",
        effect_target_digest(&binding),
        NOW,
        500,
    )
    .unwrap();
    assert_eq!(
        adapter
            .probe_checked(CredentialProbeRequest {
                schema: CONNECTOR_PORT_CONTRACT_SCHEMA.to_owned(),
                binding: binding.clone(),
                operation: "read".to_owned(),
                lease: Some(foreign_lease),
                now_unix_ms: NOW + 1,
            })
            .await
            .unwrap_err(),
        PortError::Conflict("connector_probe_credential_binding_mismatch".to_owned())
    );
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);

    // 只有能力 + 当前 binding 同时成立时才真正调用实现。
    let health = adapter.validate_binding_checked(&binding).await.unwrap();
    assert_eq!(health.status, ConnectorHealthStatus::Verified);
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ports_reject_receipts_and_observations_not_bound_to_the_permit() {
    let binding = binding();
    let payload = payload();
    let lease = lease(&binding);
    let permit = permit(&binding, &payload, lease.secret_ref.generation);

    // 回执执行了另一个 operation：合法回执也必须与许可对不上时拒绝。
    let wrong_operation = ProviderReceipt {
        operation: "write".to_owned(),
        ..receipt(&binding, ProviderOutcome::Succeeded)
    };
    assert_eq!(
        ReceiptAdapter {
            receipt: wrong_operation
        }
        .invoke_checked(permit.clone(), lease.clone(), payload.clone(), &binding)
        .await
        .unwrap_err(),
        PortError::Conflict("connector_provider_receipt_binding_invalid".to_owned())
    );

    // 回执结果回显 raw secret：在 domain 校验里被拒，且错误投影不回显哨兵。
    let leaking = ProviderReceipt {
        result: json!({"access_token": "INT07_RAW_SECRET_SENTINEL"}),
        ..receipt(&binding, ProviderOutcome::Succeeded)
    };
    let error = ReceiptAdapter { receipt: leaking }
        .invoke_checked(permit.clone(), lease.clone(), payload.clone(), &binding)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PortError::Failed("connector_provider_receipt_invalid".to_owned())
    );
    assert!(!format!("{error:?}").contains("INT07_RAW_SECRET_SENTINEL"));

    let observation_request = |receipt: ProviderReceipt| EffectObservationRequest {
        schema: CONNECTOR_OBSERVATION_REQUEST_SCHEMA.to_owned(),
        permit: permit.clone(),
        receipt,
        owner_digest: owner_digest(),
        audience_digest: audience_digest(),
        observed_at_unix_ms: NOW + 1,
    };

    // 观测来自另一份回执：payload/idempotency 相同也不能冒充本次调用结果。
    let foreign = ProviderReceipt {
        provider_receipt_id: "provider-receipt-2".to_owned(),
        ..receipt(&binding, ProviderOutcome::Succeeded)
    };
    assert_eq!(
        ForeignReceiptObserver { foreign }
            .observe_checked(observation_request(receipt(
                &binding,
                ProviderOutcome::Succeeded
            )))
            .await
            .unwrap_err(),
        PortError::Conflict("effect_observation_receipt_binding_mismatch".to_owned())
    );

    // 观测属于下一次 attempt：不能把 attempt 2 的结果记到 attempt 1。
    assert_eq!(
        LateAttemptObserver
            .observe_checked(observation_request(receipt(
                &binding,
                ProviderOutcome::Succeeded
            )))
            .await
            .unwrap_err(),
        PortError::Conflict("connector_observation_attempt_mismatch".to_owned())
    );
}

#[tokio::test]
async fn port_results_have_no_raw_secret_channel_and_reject_unknown_fields() {
    let binding = binding();
    let payload = payload();
    let lease = lease(&binding);
    let permit = permit(&binding, &payload, lease.secret_ref.generation);

    let probe = FakeProbe
        .probe_checked(CredentialProbeRequest {
            schema: CONNECTOR_PORT_CONTRACT_SCHEMA.to_owned(),
            binding: binding.clone(),
            operation: "read".to_owned(),
            lease: Some(lease.clone()),
            now_unix_ms: NOW + 1,
        })
        .await
        .unwrap();
    let mut probe_value = serde_json::to_value(&probe).unwrap();
    probe_value["secret_value"] = json!("INT07_RAW_SECRET_SENTINEL");
    assert!(serde_json::from_value::<CredentialProbeResult>(probe_value).is_err());

    let observation = FakeObserver
        .observe_checked(EffectObservationRequest {
            schema: CONNECTOR_OBSERVATION_REQUEST_SCHEMA.to_owned(),
            permit: permit.clone(),
            receipt: receipt(&binding, ProviderOutcome::Unknown),
            owner_digest: owner_digest(),
            audience_digest: audience_digest(),
            observed_at_unix_ms: NOW + 1,
        })
        .await
        .unwrap();
    let mut observation_value = serde_json::to_value(&observation).unwrap();
    observation_value["access_token"] = json!("INT07_RAW_SECRET_SENTINEL");
    assert!(serde_json::from_value::<EffectObservation>(observation_value).is_err());

    let verified = FakeWebhook
        .verify_checked(WebhookVerificationRequest {
            schema: CONNECTOR_WEBHOOK_REQUEST_SCHEMA.to_owned(),
            ingress: ingress(),
            policy: policy(),
            now_unix_ms: NOW + 1,
        })
        .await
        .unwrap();
    let mut verified_value = serde_json::to_value(&verified).unwrap();
    verified_value["authorization"] = json!("INT07_RAW_SECRET_SENTINEL");
    assert!(serde_json::from_value::<VerifiedWebhook>(verified_value).is_err());

    // 健康投影的证据字段是受校验的 digest：把 raw secret 当 evidence 传入直接失败。
    assert!(ConnectorHealth::new(
        "connector-demo",
        "binding-demo",
        ConnectorHealthStatus::Verified,
        NOW,
        Some("INT07_RAW_SECRET_SENTINEL".to_owned()),
        Vec::new(),
    )
    .is_err());
}

#[test]
fn descriptor_and_capability_registration_are_pinned_and_default_to_deny() {
    let capabilities = ConnectorAdapterCapabilities::from_iter([
        ConnectorAdapterCapability::Discover,
        ConnectorAdapterCapability::ReadOnlyProbe,
        ConnectorAdapterCapability::Invoke,
        ConnectorAdapterCapability::ObserveReceipt,
        ConnectorAdapterCapability::Cancel,
        ConnectorAdapterCapability::WebhookVerify,
        ConnectorAdapterCapability::CapabilityHandshake,
    ]);
    let descriptor =
        ConnectorAdapterDescriptor::new("connector-demo", "v1", "local_fixture", capabilities)
            .unwrap();
    assert!(descriptor.validate().is_ok());

    let mut tampered = descriptor;
    tampered.transport = "stdio_mcp".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        PortError::Failed("connector_adapter_descriptor_digest_mismatch".to_owned())
    );

    let empty = ConnectorAdapterCapabilities::default();
    assert!(!empty.supports(ConnectorAdapterCapability::Invoke));
    assert_eq!(
        empty
            .require(ConnectorAdapterCapability::ObserveReceipt)
            .unwrap_err(),
        PortError::Unavailable("connector_capability_missing:observe_receipt".to_owned())
    );
}
