//! Narrow connector transport ports.
//!
//! These traits are deliberately below the ControlPlane.  They receive only server-owned
//! admission material and return bounded observations.  They do not expose an EventStore,
//! approval store, raw credential bytes, or a second authorization path.

use crate::PortError;
use async_trait::async_trait;
use kiana_domain::{
    is_sha256_hex, json_digest, valid_extension_identifier, ConnectorBindingSnapshot,
    CredentialLease, EffectObservation, InvocationId, McpCapabilityHandshake, ProviderReceipt,
    StopReport, WorkflowEventIngress, WorkflowEventOccurrence, WorkflowEventSourcePolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub use kiana_domain::ConnectorHealthStatus;

pub const CONNECTOR_PORT_CONTRACT_SCHEMA: &str = "kiana.connector-port-contract.v1";
pub const CONNECTOR_PREPARED_PERMIT_SCHEMA: &str = "kiana.connector-prepared-permit.v1";
pub const CONNECTOR_PAYLOAD_SCHEMA: &str = "kiana.connector-canonical-payload.v1";
pub const CONNECTOR_HEALTH_SCHEMA: &str = "kiana.connector-health.v1";
pub const CONNECTOR_PROBE_RESULT_SCHEMA: &str = "kiana.connector-probe-result.v1";
pub const CONNECTOR_OBSERVATION_REQUEST_SCHEMA: &str = "kiana.connector-observation-request.v1";
pub const CONNECTOR_CANCEL_REQUEST_SCHEMA: &str = "kiana.connector-cancel-request.v1";
pub const CONNECTOR_WEBHOOK_REQUEST_SCHEMA: &str = "kiana.connector-webhook-request.v1";
pub const CONNECTOR_VERIFIED_WEBHOOK_SCHEMA: &str = "kiana.connector-verified-webhook.v1";
pub const CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA: &str = "kiana.connector-mcp-handshake-request.v1";
pub const CONNECTOR_PAYLOAD_MAX_BYTES: usize = 64 * 1024;
pub const CONNECTOR_LIMITATIONS_MAX: usize = 16;

/// Capabilities are descriptive only.  A capability never grants an account scope or an
/// approval; the checked methods below reject calls when the implementation has not registered
/// the corresponding operation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorAdapterCapability {
    Discover,
    ReadOnlyProbe,
    Invoke,
    ObserveReceipt,
    Cancel,
    WebhookVerify,
    CapabilityHandshake,
}

impl ConnectorAdapterCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::ReadOnlyProbe => "read_only_probe",
            Self::Invoke => "invoke",
            Self::ObserveReceipt => "observe_receipt",
            Self::Cancel => "cancel",
            Self::WebhookVerify => "webhook_verify",
            Self::CapabilityHandshake => "capability_handshake",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorAdapterCapabilities {
    #[serde(default)]
    pub capabilities: BTreeSet<ConnectorAdapterCapability>,
}

impl ConnectorAdapterCapabilities {
    pub fn from_iter(capabilities: impl IntoIterator<Item = ConnectorAdapterCapability>) -> Self {
        Self {
            capabilities: capabilities.into_iter().collect(),
        }
    }

    pub fn supports(&self, capability: ConnectorAdapterCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn require(&self, capability: ConnectorAdapterCapability) -> Result<(), PortError> {
        if self.supports(capability) {
            Ok(())
        } else {
            Err(PortError::Unavailable(format!(
                "connector_capability_missing:{}",
                capability.as_str()
            )))
        }
    }

    pub fn validate(&self) -> Result<(), PortError> {
        if self.capabilities.len() > 16 {
            return Err(PortError::Failed(
                "connector_capabilities_too_many".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Immutable adapter metadata.  It is a discovery projection, not an authority grant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorAdapterDescriptor {
    pub schema: String,
    pub connector_id: String,
    pub version: String,
    pub transport: String,
    pub capabilities: ConnectorAdapterCapabilities,
    pub descriptor_digest: String,
}

impl ConnectorAdapterDescriptor {
    pub fn new(
        connector_id: impl Into<String>,
        version: impl Into<String>,
        transport: impl Into<String>,
        capabilities: ConnectorAdapterCapabilities,
    ) -> Result<Self, PortError> {
        let mut descriptor = Self {
            schema: CONNECTOR_PORT_CONTRACT_SCHEMA.to_owned(),
            connector_id: connector_id.into(),
            version: version.into(),
            transport: transport.into(),
            capabilities,
            descriptor_digest: String::new(),
        };
        descriptor.descriptor_digest = descriptor.digest();
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_PORT_CONTRACT_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.version)
            || self.transport.trim().is_empty()
            || self.transport.len() > 128
            || self.transport.contains(['\0', '\r', '\n'])
        {
            return Err(PortError::Failed(
                "connector_adapter_descriptor_invalid".to_owned(),
            ));
        }
        self.capabilities.validate()?;
        if self.descriptor_digest != self.digest() {
            return Err(PortError::Failed(
                "connector_adapter_descriptor_digest_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "version": self.version,
            "transport": self.transport,
            "capabilities": self.capabilities,
        }))
    }
}

/// Canonical payload supplied after schema, data-class and policy validation.  The adapter may
/// need the value to perform an operation, but the value is never returned by a port as
/// credential material and is not accepted as an authority input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalConnectorPayload {
    pub schema: String,
    pub value: Value,
    pub payload_digest: String,
}

impl CanonicalConnectorPayload {
    pub fn new(value: Value) -> Result<Self, PortError> {
        let mut payload = Self {
            schema: CONNECTOR_PAYLOAD_SCHEMA.to_owned(),
            value,
            payload_digest: String::new(),
        };
        payload.payload_digest = json_digest(&payload.value);
        payload.validate()?;
        Ok(payload)
    }

    pub fn validate(&self) -> Result<(), PortError> {
        let bytes = serde_json::to_vec(&self.value)
            .map_err(|_| PortError::Failed("connector_payload_invalid".to_owned()))?;
        if self.schema != CONNECTOR_PAYLOAD_SCHEMA
            || bytes.len() > CONNECTOR_PAYLOAD_MAX_BYTES
            || self.payload_digest != json_digest(&self.value)
        {
            return Err(PortError::Failed(
                "connector_canonical_payload_invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Effect-time permit produced by the ControlPlane.  The adapter cannot mint or widen this
/// object; it only receives the already bound connector/version/account/operation and digests.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorPreparedPermit {
    pub schema: String,
    pub connector_id: String,
    pub connector_version: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub action_digest: String,
    pub payload_digest: String,
    pub idempotency_key_digest: String,
    pub authority_epoch: u64,
    pub policy_revision: String,
    pub binding_revision: u64,
    pub credential_generation: u64,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub permit_digest: String,
}

impl ConnectorPreparedPermit {
    pub fn validate(&self, now_unix_ms: Option<u64>) -> Result<(), PortError> {
        if self.schema != CONNECTOR_PREPARED_PERMIT_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.connector_version)
            || !valid_extension_identifier(&self.binding_id)
            || !valid_extension_identifier(&self.account_id)
            || !valid_extension_identifier(&self.operation)
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || self.authority_epoch == 0
            || self.policy_revision.trim().is_empty()
            || self.policy_revision.len() > 256
            || self.binding_revision == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.idempotency_key_digest)
            || !valid_digest(&self.permit_digest)
            || self.permit_digest != self.digest()
            || now_unix_ms
                .is_some_and(|now| now < self.issued_at_unix_ms || now >= self.expires_at_unix_ms)
        {
            return Err(PortError::Conflict(
                "connector_prepared_permit_invalid_or_expired".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "connector_version": self.connector_version,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "action_digest": self.action_digest,
            "payload_digest": self.payload_digest,
            "idempotency_key_digest": self.idempotency_key_digest,
            "authority_epoch": self.authority_epoch,
            "policy_revision": self.policy_revision,
            "binding_revision": self.binding_revision,
            "credential_generation": self.credential_generation,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }

    fn validate_for_binding(
        &self,
        binding: &ConnectorBindingSnapshot,
        payload: &CanonicalConnectorPayload,
        lease: &CredentialLease,
    ) -> Result<(), PortError> {
        self.validate(None)?;
        binding
            .validate()
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        binding
            .operation(&self.operation)
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        payload.validate()?;
        if self.connector_id != binding.definition.connector_id
            || self.connector_version != binding.definition.version
            || self.binding_id != binding.binding.binding_id
            || self.account_id != binding.binding.account_id
            || self.binding_revision != binding.revision
            || self.payload_digest != payload.payload_digest
            || binding.binding.credential_ref.as_ref() != Some(&lease.secret_ref)
            || lease.secret_ref.generation != self.credential_generation
        {
            return Err(PortError::Conflict(
                "connector_prepared_permit_binding_mismatch".to_owned(),
            ));
        }
        lease
            .validate_at(self.issued_at_unix_ms)
            .map_err(|error| PortError::Conflict(format!("connector_credential_lease:{error}")))
    }
}

/// Health is a redacted status projection.  Provider response bodies and credential material do
/// not cross this port.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHealth {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub status: ConnectorHealthStatus,
    pub checked_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ConnectorHealth {
    pub fn new(
        connector_id: impl Into<String>,
        binding_id: impl Into<String>,
        status: ConnectorHealthStatus,
        checked_at_unix_ms: u64,
        evidence_digest: Option<String>,
        limitations: Vec<String>,
    ) -> Result<Self, PortError> {
        let health = Self {
            schema: CONNECTOR_HEALTH_SCHEMA.to_owned(),
            connector_id: connector_id.into(),
            binding_id: binding_id.into(),
            status,
            checked_at_unix_ms,
            evidence_digest,
            limitations,
        };
        health.validate()?;
        Ok(health)
    }

    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_HEALTH_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.binding_id)
            || self.checked_at_unix_ms == 0
            || self.limitations.len() > CONNECTOR_LIMITATIONS_MAX
            || self.limitations.iter().any(|value| {
                value.trim().is_empty() || value.len() > 512 || value.contains(['\0', '\r', '\n'])
            })
            || self
                .evidence_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return Err(PortError::Failed("connector_health_invalid".to_owned()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialProbeRequest {
    pub schema: String,
    pub binding: ConnectorBindingSnapshot,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<CredentialLease>,
    pub now_unix_ms: u64,
}

impl CredentialProbeRequest {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_PORT_CONTRACT_SCHEMA || self.now_unix_ms == 0 {
            return Err(PortError::Failed(
                "connector_probe_request_invalid".to_owned(),
            ));
        }
        self.binding
            .validate()
            .map_err(|error| PortError::Conflict(error.to_owned()))?;
        if self
            .binding
            .operation(&self.operation)
            .map_err(|error| PortError::Conflict(format!("connector_probe_operation:{error}")))?
            .risk()
            != kiana_domain::RiskLevel::ReadOnly
        {
            return Err(PortError::Failed(
                "connector_probe_must_be_read_only".to_owned(),
            ));
        }
        if let Some(lease) = &self.lease {
            lease.validate_at(self.now_unix_ms).map_err(|error| {
                PortError::Conflict(format!("connector_credential_lease:{error}"))
            })?;
            if self.binding.binding.credential_ref.as_ref() != Some(&lease.secret_ref) {
                return Err(PortError::Conflict(
                    "connector_probe_credential_binding_mismatch".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialProbeResult {
    pub schema: String,
    pub status: ConnectorHealthStatus,
    pub health: ConnectorHealth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
}

/// Server-owned input for a stdio MCP capability handshake.  It contains no command, URL,
/// headers or scope grant; the trusted registry resolves those after the ControlPlane permit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCapabilityHandshakeRequest {
    pub schema: String,
    pub binding: ConnectorBindingSnapshot,
    pub server: String,
    pub session_ref: String,
}

impl McpCapabilityHandshakeRequest {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA
            || !valid_extension_identifier(&self.server)
            || self.session_ref.trim().is_empty()
            || self.session_ref.len() > 256
            || self.session_ref.chars().any(char::is_control)
        {
            return Err(PortError::Failed(
                "connector_mcp_handshake_request_invalid".to_owned(),
            ));
        }
        self.binding
            .validate()
            .map_err(|error| PortError::Conflict(error.to_owned()))
    }
}

impl CredentialProbeResult {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_PROBE_RESULT_SCHEMA
            || self.status != self.health.status
            || self
                .credential_generation
                .is_some_and(|generation| generation == 0)
            || self
                .evidence_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return Err(PortError::Failed(
                "connector_probe_result_invalid".to_owned(),
            ));
        }
        self.health.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectObservationRequest {
    pub schema: String,
    pub permit: ConnectorPreparedPermit,
    pub receipt: ProviderReceipt,
    pub owner_digest: String,
    pub audience_digest: String,
    pub observed_at_unix_ms: u64,
}

impl EffectObservationRequest {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_OBSERVATION_REQUEST_SCHEMA
            || !valid_digest(&self.owner_digest)
            || !valid_digest(&self.audience_digest)
            || self.observed_at_unix_ms == 0
            || self.receipt.connector_id != self.permit.connector_id
            || self.receipt.binding_id != self.permit.binding_id
            || self.receipt.account_id != self.permit.account_id
            || self.receipt.operation != self.permit.operation
            || json_digest(&serde_json::json!({
                "idempotency_key": self.receipt.idempotency_key
            })) != self.permit.idempotency_key_digest
        {
            return Err(PortError::Conflict(
                "connector_observation_binding_mismatch".to_owned(),
            ));
        }
        self.permit.validate(None)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCancelRequest {
    pub schema: String,
    pub permit: ConnectorPreparedPermit,
    pub reason: String,
    pub requested_at_unix_ms: u64,
}

impl ConnectorCancelRequest {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_CANCEL_REQUEST_SCHEMA
            || self.reason.trim().is_empty()
            || self.reason.len() > 512
            || self.reason.contains(['\0', '\r', '\n'])
            || self.requested_at_unix_ms == 0
        {
            return Err(PortError::Failed(
                "connector_cancel_request_invalid".to_owned(),
            ));
        }
        self.permit.validate(None)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookVerificationRequest {
    pub schema: String,
    pub ingress: WorkflowEventIngress,
    pub policy: WorkflowEventSourcePolicy,
    pub now_unix_ms: u64,
}

impl WebhookVerificationRequest {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_WEBHOOK_REQUEST_SCHEMA || self.now_unix_ms == 0 {
            return Err(PortError::Failed(
                "connector_webhook_request_invalid".to_owned(),
            ));
        }
        self.ingress
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        self.policy
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))
    }
}

/// A verified webhook contains only the occurrence projection.  The signed payload remains at
/// the ingress/artifact boundary and cannot be returned by this port as a capability request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedWebhook {
    pub schema: String,
    pub occurrence: WorkflowEventOccurrence,
    pub verified_at_unix_ms: u64,
    pub verification_digest: String,
}

impl VerifiedWebhook {
    pub fn from_occurrence(
        occurrence: WorkflowEventOccurrence,
        verified_at_unix_ms: u64,
    ) -> Result<Self, PortError> {
        occurrence
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        let mut verified = Self {
            schema: CONNECTOR_VERIFIED_WEBHOOK_SCHEMA.to_owned(),
            occurrence,
            verified_at_unix_ms,
            verification_digest: String::new(),
        };
        verified.verification_digest = verified.digest();
        verified.validate()?;
        Ok(verified)
    }

    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_VERIFIED_WEBHOOK_SCHEMA
            || self.verified_at_unix_ms == 0
            || !valid_digest(&self.verification_digest)
            || self.verification_digest != self.digest()
        {
            return Err(PortError::Failed(
                "connector_verified_webhook_invalid".to_owned(),
            ));
        }
        self.occurrence
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "occurrence": self.occurrence,
            "verified_at_unix_ms": self.verified_at_unix_ms,
        }))
    }
}

/// Connector transport boundary.  The implementation receives no EventStore or approval
/// authority.  Every default operation is unsupported, and checked wrappers require an explicit
/// registered capability before invoking an implementation method.
#[async_trait]
pub trait ConnectorAdapter: Send + Sync {
    fn capabilities(&self) -> ConnectorAdapterCapabilities {
        ConnectorAdapterCapabilities::default()
    }

    async fn discover(
        &self,
        _binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorAdapterDescriptor, PortError> {
        Err(PortError::Unavailable(
            "connector_discover_unsupported".to_owned(),
        ))
    }

    async fn validate_binding(
        &self,
        _binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorHealth, PortError> {
        Err(PortError::Unavailable(
            "connector_binding_validation_unsupported".to_owned(),
        ))
    }

    async fn invoke(
        &self,
        _permit: ConnectorPreparedPermit,
        _lease: CredentialLease,
        _payload: CanonicalConnectorPayload,
    ) -> Result<ProviderReceipt, PortError> {
        Err(PortError::Unavailable(
            "connector_invoke_unsupported".to_owned(),
        ))
    }

    async fn cancel(&self, _request: ConnectorCancelRequest) -> Result<StopReport, PortError> {
        Err(PortError::Unavailable(
            "connector_cancel_unsupported".to_owned(),
        ))
    }

    async fn capability_handshake(
        &self,
        _request: McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        Err(PortError::Unavailable(
            "connector_capability_handshake_unsupported".to_owned(),
        ))
    }

    async fn discover_checked(
        &self,
        binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorAdapterDescriptor, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::Discover)?;
        let descriptor = self.discover(binding).await?;
        descriptor.validate()?;
        Ok(descriptor)
    }

    async fn validate_binding_checked(
        &self,
        binding: &ConnectorBindingSnapshot,
    ) -> Result<ConnectorHealth, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::ReadOnlyProbe)?;
        let health = self.validate_binding(binding).await?;
        health.validate()?;
        Ok(health)
    }

    async fn invoke_checked(
        &self,
        permit: ConnectorPreparedPermit,
        lease: CredentialLease,
        payload: CanonicalConnectorPayload,
        binding: &ConnectorBindingSnapshot,
    ) -> Result<ProviderReceipt, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::Invoke)?;
        permit.validate_for_binding(binding, &payload, &lease)?;
        let receipt = self.invoke(permit.clone(), lease, payload).await?;
        validate_receipt_for_permit(&receipt, binding, &permit)?;
        Ok(receipt)
    }

    async fn cancel_checked(
        &self,
        request: ConnectorCancelRequest,
    ) -> Result<StopReport, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::Cancel)?;
        request.validate()?;
        let report = self.cancel(request).await?;
        report
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        Ok(report)
    }

    async fn capability_handshake_checked(
        &self,
        request: McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        self.capabilities()
            .require(ConnectorAdapterCapability::CapabilityHandshake)?;
        request.validate()?;
        let handshake = self.capability_handshake(request).await?;
        handshake.validate().map_err(PortError::Failed)?;
        Ok(handshake)
    }
}

/// Effect observation is intentionally a separate port from dispatch.  It returns a typed,
/// digest-only observation and cannot append facts itself.
#[async_trait]
pub trait EffectObserver: Send + Sync {
    fn supports_observation(&self) -> bool {
        false
    }

    async fn observe(
        &self,
        _request: EffectObservationRequest,
    ) -> Result<EffectObservation, PortError> {
        Err(PortError::Unavailable(
            "connector_effect_observation_unsupported".to_owned(),
        ))
    }

    async fn observe_checked(
        &self,
        request: EffectObservationRequest,
    ) -> Result<EffectObservation, PortError> {
        if !self.supports_observation() {
            return Err(PortError::Unavailable(
                "connector_capability_missing:observe_receipt".to_owned(),
            ));
        }
        request.validate()?;
        let expected_attempt = request.permit.attempt;
        let expected_invocation_id = request.permit.invocation_id;
        let observation = self.observe(request).await?;
        observation
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        if observation.attempt != expected_attempt
            || observation.invocation_id != expected_invocation_id
        {
            return Err(PortError::Conflict(
                "connector_observation_attempt_mismatch".to_owned(),
            ));
        }
        Ok(observation)
    }
}

/// Read-only health/credential probe.  A probe returns status and evidence metadata, never a
/// token, header or secret value.
#[async_trait]
pub trait CredentialProbe: Send + Sync {
    fn supports_probe(&self) -> bool {
        false
    }

    async fn probe(
        &self,
        _request: CredentialProbeRequest,
    ) -> Result<CredentialProbeResult, PortError> {
        Err(PortError::Unavailable(
            "connector_credential_probe_unsupported".to_owned(),
        ))
    }

    async fn probe_checked(
        &self,
        request: CredentialProbeRequest,
    ) -> Result<CredentialProbeResult, PortError> {
        if !self.supports_probe() {
            return Err(PortError::Unavailable(
                "connector_capability_missing:read_only_probe".to_owned(),
            ));
        }
        request.validate()?;
        let expected_connector_id = request.binding.definition.connector_id.clone();
        let expected_binding_id = request.binding.binding.binding_id.clone();
        let result = self.probe(request).await?;
        result.validate()?;
        if result.health.connector_id != expected_connector_id
            || result.health.binding_id != expected_binding_id
        {
            return Err(PortError::Conflict(
                "connector_probe_health_binding_mismatch".to_owned(),
            ));
        }
        Ok(result)
    }
}

/// Webhook verification returns an occurrence projection, so a verified inbound event still has
/// to enter the normal Workflow/ControlPlane path before it can create a run or capability.
#[async_trait]
pub trait WebhookVerifier: Send + Sync {
    fn supports_webhook_verification(&self) -> bool {
        false
    }

    async fn verify(
        &self,
        _request: WebhookVerificationRequest,
    ) -> Result<VerifiedWebhook, PortError> {
        Err(PortError::Unavailable(
            "connector_webhook_verification_unsupported".to_owned(),
        ))
    }

    async fn verify_checked(
        &self,
        request: WebhookVerificationRequest,
    ) -> Result<VerifiedWebhook, PortError> {
        if !self.supports_webhook_verification() {
            return Err(PortError::Unavailable(
                "connector_capability_missing:webhook_verify".to_owned(),
            ));
        }
        request.validate()?;
        let expected_source_id = request.ingress.source_id.clone();
        let expected_event_id = request.ingress.event_id.clone();
        let expected_project_id = request.ingress.project_id.clone();
        let expected_trigger_id = request.ingress.trigger_id.clone();
        let expected_payload_digest = request.ingress.payload_digest.clone();
        let expected_policy_digest = request.policy.policy_digest.clone();
        let verified = self.verify(request).await?;
        verified.validate()?;
        if verified.occurrence.source_id != expected_source_id
            || verified.occurrence.event_id != expected_event_id
            || verified.occurrence.project_id != expected_project_id
            || verified.occurrence.trigger_id != expected_trigger_id
            || verified.occurrence.payload_digest != expected_payload_digest
            || verified.occurrence.policy_digest != expected_policy_digest
        {
            return Err(PortError::Conflict(
                "connector_webhook_occurrence_binding_mismatch".to_owned(),
            ));
        }
        Ok(verified)
    }
}

fn validate_receipt_for_permit(
    receipt: &ProviderReceipt,
    binding: &ConnectorBindingSnapshot,
    permit: &ConnectorPreparedPermit,
) -> Result<(), PortError> {
    if receipt.schema != "kiana.provider-receipt.v1"
        || receipt.connector_id != binding.definition.connector_id
        || receipt.binding_id != binding.binding.binding_id
        || receipt.account_id != binding.binding.account_id
        || receipt.operation != permit.operation
        || json_digest(&serde_json::json!({
            "idempotency_key": receipt.idempotency_key
        })) != permit.idempotency_key_digest
        || !is_sha256_hex(&receipt.final_payload_sha256)
    {
        return Err(PortError::Conflict(
            "connector_provider_receipt_binding_invalid".to_owned(),
        ));
    }
    if !valid_extension_identifier(&receipt.provider_receipt_id)
        || receipt.source.trim().is_empty()
        || receipt.source.len() > 128
    {
        return Err(PortError::Failed(
            "connector_provider_receipt_invalid".to_owned(),
        ));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
