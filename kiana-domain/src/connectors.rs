//! Connector contracts. External transports remain closed; stdio MCP is the only remote-style
//! adapter admitted by the INT-10 source slice and still runs inside the confined child boundary.

use crate::{
    connector_fixture_hash_valid, is_sha256_hex, json_digest, scan_secret_sentinels,
    valid_extension_identifier, valid_extension_path, ConnectorDispatchLifecycle,
    ConnectorDispatchStage, ConnectorReconciliationCase, ConnectorReconciliationState,
    EffectObservation, InvocationId, RiskLevel, SecretRef, SecretScanChannel,
    CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CONNECTOR_MANAGE_OPERATION: &str = "connector.manage";
pub const CONNECTOR_INVOKE_OPERATION: &str = "connector.invoke";
/// Public typed reconciliation command. Core maps it to the existing management handler with
/// `action=reconcile`; no second Broker or adapter path is introduced.
pub const CONNECTOR_RECONCILE_OPERATION: &str = "connector.reconcile";
/// Read-only binding health command. The command only authorizes an adapter probe; it never
/// grants an invocation permit or performs an external effect.
pub const CONNECTOR_HEALTH_OPERATION: &str = "connector.health";
/// Read-only stdio MCP initialize/tools-list handshake. It only publishes untrusted capability
/// metadata and never grants a server advertised scope.
pub const CONNECTOR_MCP_HANDSHAKE_OPERATION: &str = "connector.mcp_handshake";
pub const CONNECTOR_STREAM: &str = "connector_registry";
pub const CONNECTOR_HEALTH_EVENT_KIND: &str = "connector.health_checked";
pub const CONNECTOR_MCP_HANDSHAKE_EVENT_KIND: &str = "connector.mcp_handshake";
pub const CONNECTOR_HEALTH_FACT_SCHEMA: &str = "kiana.connector-health-fact.v1";
pub const CONNECTOR_HEALTH_PROJECTION_SCHEMA: &str = "kiana.connector-health-projection.v1";
pub const CONNECTOR_HEALTH_MAX_LIMITATIONS: usize = 16;
/// The only public error code a probe may emit after raw provider text was detected. Callers can
/// distinguish the status class but never recover the response body, JSON error document or
/// credential value from the persisted fact.
pub const CONNECTOR_HEALTH_REDACTED_ERROR_CODE: &str = "connector_probe_failed";
pub const PROVIDER_RECEIPT_SCHEMA: &str = "kiana.provider-receipt.v1";
pub const PROVIDER_RECEIPT_MAX_RESULT_BYTES: usize = 64 * 1024;
/// Server-owned expectation that a receipt and its effect observation must satisfy.  It is built
/// from the authorized binding/permit (never from the receipt under validation) so a receipt that
/// belongs to another binding, account, operation, attempt or idempotency key fails closed.
pub const PROVIDER_RECEIPT_BINDING_SCHEMA: &str = "kiana.provider-receipt-binding.v1";
/// Digest-only, released view of a receipt.  Raw provider response material and free-form result
/// values stay out of receipts, events, logs and this projection.
pub const PROVIDER_RECEIPT_PROJECTION_SCHEMA: &str = "kiana.provider-receipt-projection.v1";
const PROVIDER_RECEIPT_FORBIDDEN_RESULT_KEYS: &[&str] = &[
    "authorization",
    "headers",
    "provider_response",
    "raw_error",
    "raw_provider_response",
    "raw_response",
    "raw_response_body",
    "response_body",
];

/// Safe status vocabulary shared by adapter probes, EventLog facts and query/UI projections.
/// No variant carries provider response material or credential values.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorHealthStatus {
    Verified,
    ConnectivityOnly,
    CredentialInvalid,
    ScopeInsufficient,
    EndpointUnreachable,
    ProviderError,
    Unsupported,
}

impl ConnectorHealthStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::ConnectivityOnly => "connectivity_only",
            Self::CredentialInvalid => "credential_invalid",
            Self::ScopeInsufficient => "scope_insufficient",
            Self::EndpointUnreachable => "endpoint_unreachable",
            Self::ProviderError => "provider_error",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Map only stable provider/transport error codes into public health classes. Raw response text
/// must be discarded before reaching this function; unknown codes remain `provider_error`.
///
/// Scope outcomes are matched before credential outcomes on purpose. A provider that answers
/// `403`/`forbidden`/`permission_denied` because the credential lacks a grant must never be
/// reported as an invalid credential; otherwise an operator would rotate a valid secret and the
/// real missing-scope condition would stay hidden.
pub fn classify_connector_health_error(error_code: &str) -> ConnectorHealthStatus {
    let normalized = error_code
        .split(':')
        .next()
        .unwrap_or(error_code)
        .trim()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "scope_insufficient"
        | "missing_scope"
        | "insufficient_scope"
        | "scope_missing"
        | "scope_denied"
        | "invalid_scope"
        | "forbidden"
        | "permission_denied"
        | "access_denied"
        | "insufficient_permissions" => ConnectorHealthStatus::ScopeInsufficient,
        "credential_invalid"
        | "invalid_credential"
        | "unauthorized"
        | "invalid_token"
        | "authentication_failed"
        | "invalid_api_key" => ConnectorHealthStatus::CredentialInvalid,
        "endpoint_unreachable"
        | "dns_failed"
        | "connection_refused"
        | "timeout"
        | "connect_timeout"
        | "tls_failed" => ConnectorHealthStatus::EndpointUnreachable,
        "unsupported" | "transport_unsupported" | "probe_unsupported" => {
            ConnectorHealthStatus::Unsupported
        }
        _ => ConnectorHealthStatus::ProviderError,
    }
}

/// Reduce an adapter/probe error string to the only shape allowed into a health fact.
///
/// A stable code keeps its first `:` segment when that segment is short, lowercase and made of
/// ASCII identifier characters. Anything else - a non-JSON error body, an HTML page, a multiline
/// message, a URL, an Authorization header or a credential-shaped token - collapses to
/// [`CONNECTOR_HEALTH_REDACTED_ERROR_CODE`]. This helper never returns any part of the raw text
/// for those inputs, so a probe failure cannot leak provider material into EventLog or UI.
pub fn redacted_connector_probe_error_code(error: &str) -> String {
    // Multiline bodies, NUL and other control characters are never stable codes, even when the
    // first segment happens to look like one.
    if error.chars().any(char::is_control) {
        return CONNECTOR_HEALTH_REDACTED_ERROR_CODE.to_owned();
    }
    let projection = crate::project_redacted_error(SecretScanChannel::Event, error);
    if projection.redacted {
        return CONNECTOR_HEALTH_REDACTED_ERROR_CODE.to_owned();
    }
    let normalized = error
        .split(':')
        .next()
        .unwrap_or(error)
        .trim()
        .to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.len() > 128
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return CONNECTOR_HEALTH_REDACTED_ERROR_CODE.to_owned();
    }
    normalized
}

/// Classify a probe failure and return the bounded code that may be persisted.
///
/// Fixture-level failures are mapped here so every caller applies the same classification. A
/// failed probe is an observation only: it never revokes a credential, a binding or a principal.
pub fn classify_connector_probe_error(error: &str) -> (ConnectorHealthStatus, String) {
    let code = redacted_connector_probe_error_code(error);
    let status = match code.as_str() {
        "connector_fixture_hash_mismatch"
        | "connector_evidence_path_invalid"
        | "connector_fixture_join_failed"
        | "connector_project_invalid" => ConnectorHealthStatus::EndpointUnreachable,
        "connector_fixture_external_effect_denied" => ConnectorHealthStatus::Unsupported,
        _ => classify_connector_health_error(&code),
    };
    (status, code)
}

/// Persisted, redacted health evidence. This is a fact projection, not a credential or provider
/// response cache. The only error field allowed through the boundary is a stable classification.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHealthFact {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub status: ConnectorHealthStatus,
    pub probe_kind: String,
    pub checked_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub binding_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_generation: Option<u64>,
    pub source: String,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ConnectorHealthFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        connector_id: impl Into<String>,
        binding_id: impl Into<String>,
        status: ConnectorHealthStatus,
        probe_kind: impl Into<String>,
        checked_at_unix_ms: u64,
        evidence_digest: Option<String>,
        error_code: Option<String>,
        binding_revision: u64,
        credential_generation: Option<u64>,
        source: impl Into<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let fact = Self {
            schema: CONNECTOR_HEALTH_FACT_SCHEMA.to_owned(),
            connector_id: connector_id.into(),
            binding_id: binding_id.into(),
            status,
            probe_kind: probe_kind.into(),
            checked_at_unix_ms,
            evidence_digest,
            error_code,
            binding_revision,
            credential_generation,
            source: source.into(),
            limitations,
        };
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_HEALTH_FACT_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.binding_id)
            || self.probe_kind != "read_only"
            || self.checked_at_unix_ms == 0
            || self.binding_revision == 0
            || !valid_extension_identifier(&self.source)
            || self.limitations.len() > CONNECTOR_HEALTH_MAX_LIMITATIONS
            || self.limitations.iter().any(|value| {
                value.trim().is_empty() || value.len() > 256 || value.contains(['\0', '\r', '\n'])
            })
            || self.error_code.as_deref().is_some_and(|value| {
                value.trim().is_empty()
                    || value.len() > 128
                    || value.contains(['\0', '\r', '\n'])
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
                    })
            })
            || self.evidence_digest.as_deref().is_some_and(|value| {
                value
                    .strip_prefix("sha256:")
                    .is_none_or(|hex| !is_sha256_hex(hex))
            })
            || self
                .credential_generation
                .is_some_and(|generation| generation == 0)
        {
            return Err("connector_health_fact_invalid".to_owned());
        }
        // A successful or connectivity-only observation has no failure code. Keeping the
        // dimensions separate prevents a stale error from masquerading as part of a new success.
        if matches!(
            self.status,
            ConnectorHealthStatus::Verified | ConnectorHealthStatus::ConnectivityOnly
        ) && self.error_code.is_some()
        {
            return Err("connector_health_status_conflict".to_owned());
        }
        // Limitations are free-form enough to become a covert raw-error channel; reject secret
        // sentinels at the fact boundary even if an adapter forgot to redact before constructing.
        if self
            .limitations
            .iter()
            .any(|value| scan_secret_sentinels(SecretScanChannel::Event, value).is_err())
        {
            return Err("connector_health_limitation_invalid".to_owned());
        }
        if matches!(self.status, ConnectorHealthStatus::Verified) && self.evidence_digest.is_none()
        {
            return Err("connector_health_verified_evidence_required".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorEffect {
    ReadOnly,
    Write,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorOperation {
    pub effect: ConnectorEffect,
    pub required_scope: String,
    #[serde(default)]
    pub data_classes: BTreeSet<String>,
}

impl ConnectorOperation {
    pub fn risk(&self) -> RiskLevel {
        match self.effect {
            ConnectorEffect::ReadOnly => RiskLevel::ReadOnly,
            ConnectorEffect::Write => RiskLevel::ExternalSideEffect,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorDefinition {
    pub schema: String,
    pub connector_id: String,
    pub version: String,
    pub provider_id: String,
    /// The adapter is server-owned. `stdio_mcp` is constrained to a local child process and never
    /// accepts an HTTP endpoint or a server supplied scope.
    pub adapter: String,
    pub operations: BTreeMap<String, ConnectorOperation>,
    pub rate_limit_per_minute: u32,
    pub idempotency_required: bool,
    pub reconciliation_required: bool,
    pub data_processing: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountBinding {
    pub schema: String,
    pub binding_id: String,
    pub connector_id: String,
    pub account_id: String,
    pub read_scopes: BTreeSet<String>,
    pub write_scopes: BTreeSet<String>,
    pub fixture_path: String,
    pub fixture_sha256: String,
    /// Optional opaque reference resolved only by a trusted adapter at its effect boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<SecretRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorBindingSnapshot {
    pub definition: ConnectorDefinition,
    pub binding: AccountBinding,
    pub project_root: String,
    pub revision: u64,
    pub status: String,
}

impl ConnectorBindingSnapshot {
    pub fn validate(&self) -> Result<(), &'static str> {
        let definition = &self.definition;
        let binding = &self.binding;
        if definition.schema != "kiana.connector-definition.v1"
            || binding.schema != "kiana.account-binding.v1"
            || definition.connector_id != binding.connector_id
            || [
                definition.connector_id.as_str(),
                definition.version.as_str(),
                definition.provider_id.as_str(),
                binding.binding_id.as_str(),
                binding.account_id.as_str(),
            ]
            .into_iter()
            .any(|id| !valid_extension_identifier(id))
            || !valid_extension_path(&binding.fixture_path)
            || !connector_fixture_hash_valid(&binding.fixture_sha256)
            || self.project_root.trim().is_empty()
            || !matches!(self.status.as_str(), "active" | "revoked")
        {
            return Err("connector_binding_invalid");
        }
        if !matches!(definition.adapter.as_str(), "local_fixture" | "stdio_mcp")
            || definition.data_processing != "local_only"
        {
            return Err("connector_transport_not_supported");
        }
        if definition.operations.is_empty()
            || definition.operations.len() > 32
            || definition.rate_limit_per_minute == 0
            || definition.rate_limit_per_minute > 1000
            || !definition.idempotency_required
            || !definition.reconciliation_required
        {
            return Err("connector_controls_required");
        }
        if definition.operations.iter().any(|(name, operation)| {
            !valid_extension_identifier(name)
                || !valid_extension_identifier(&operation.required_scope)
                || operation.data_classes.len() > 32
        }) || binding
            .read_scopes
            .iter()
            .chain(binding.write_scopes.iter())
            .any(|scope| !valid_extension_identifier(scope))
        {
            return Err("connector_scope_invalid");
        }
        if let Some(reference) = &binding.credential_ref {
            if reference.validate().is_err()
                || reference.store != "env"
                || reference.key.is_empty()
                || !reference
                    .key
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
                || reference.purpose != crate::CONNECTOR_CREDENTIAL_PURPOSE
                || reference.audience
                    != crate::connector_credential_audience(
                        &definition.connector_id,
                        &definition.version,
                    )
            {
                return Err("connector_credential_reference_invalid");
            }
        }
        Ok(())
    }

    pub fn operation(&self, name: &str) -> Result<&ConnectorOperation, &'static str> {
        self.validate()?;
        if self.status != "active" {
            return Err("connector_binding_revoked");
        }
        let operation = self
            .definition
            .operations
            .get(name)
            .ok_or("connector_operation_unregistered")?;
        let scopes = match operation.effect {
            ConnectorEffect::ReadOnly => &self.binding.read_scopes,
            ConnectorEffect::Write => &self.binding.write_scopes,
        };
        if !scopes.contains(&operation.required_scope) {
            return Err("connector_account_scope_denied");
        }
        Ok(operation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderOutcome {
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReceipt {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub idempotency_key: String,
    pub final_payload_sha256: String,
    pub provider_receipt_id: String,
    pub outcome: ProviderOutcome,
    pub source: String,
    pub result: Value,
}

/// Receipts are released through `projection()`; `Debug` must not publish the free-form result or
/// any raw provider material that slipped past the constructor.
impl std::fmt::Debug for ProviderReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderReceipt")
            .field("schema", &self.schema)
            .field("connector_id", &self.connector_id)
            .field("binding_id", &self.binding_id)
            .field("account_id", &self.account_id)
            .field("operation", &self.operation)
            .field(
                "idempotency_key_digest",
                &json_digest(&serde_json::json!({"idempotency_key": self.idempotency_key})),
            )
            .field("final_payload_sha256", &self.final_payload_sha256)
            .field("provider_receipt_id", &self.provider_receipt_id)
            .field("outcome", &self.outcome)
            .field("source", &self.source)
            .field("result_digest", &json_digest(&self.result))
            .finish()
    }
}

/// Server-owned expectation for one connector attempt.  Every field is derived from the authorized
/// binding/permit/invocation, never from the receipt or observation under validation; the digest
/// self-proof makes a caller-computed expectation detectable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReceiptBinding {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub idempotency_key_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_digest: Option<String>,
    pub binding_digest: String,
}

impl ProviderReceiptBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        connector_id: impl Into<String>,
        binding_id: impl Into<String>,
        account_id: impl Into<String>,
        operation: impl Into<String>,
        invocation_id: InvocationId,
        attempt: u32,
        idempotency_key_digest: impl Into<String>,
        payload_sha256: Option<String>,
        owner_digest: Option<String>,
        audience_digest: Option<String>,
    ) -> Result<Self, String> {
        let payload_sha256 = match payload_sha256 {
            Some(value) => Some(
                normalize_provider_payload_hash(&value)
                    .ok_or_else(|| "provider_receipt_binding_header_invalid".to_owned())?,
            ),
            None => None,
        };
        let mut binding = Self {
            schema: PROVIDER_RECEIPT_BINDING_SCHEMA.to_owned(),
            connector_id: connector_id.into(),
            binding_id: binding_id.into(),
            account_id: account_id.into(),
            operation: operation.into(),
            invocation_id,
            attempt,
            idempotency_key_digest: idempotency_key_digest.into(),
            payload_sha256,
            owner_digest,
            audience_digest,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_RECEIPT_BINDING_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.binding_id)
            || !valid_extension_identifier(&self.account_id)
            || !valid_extension_identifier(&self.operation)
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || !valid_prefixed_digest(&self.idempotency_key_digest)
            || !valid_prefixed_digest(&self.binding_digest)
            || self.payload_sha256.as_deref().is_some_and(|value| {
                normalize_provider_payload_hash(value).as_deref() != Some(value)
            })
            || self
                .owner_digest
                .as_deref()
                .is_some_and(|value| !valid_prefixed_digest(value))
            || self
                .audience_digest
                .as_deref()
                .is_some_and(|value| !valid_prefixed_digest(value))
        {
            return Err("provider_receipt_binding_header_invalid".to_owned());
        }
        if self.binding_digest != self.digest() {
            return Err("provider_receipt_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "idempotency_key_digest": self.idempotency_key_digest,
            "payload_sha256": self.payload_sha256,
            "owner_digest": self.owner_digest,
            "audience_digest": self.audience_digest,
        }))
    }
}

/// Digest-only released view of a validated receipt.  It never carries `result` or raw provider
/// response material, and the projection digest is recomputed after every release.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReceiptProjection {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub idempotency_key_digest: String,
    pub payload_sha256: String,
    pub provider_receipt_id: String,
    pub outcome: ProviderOutcome,
    pub source: String,
    pub result_digest: String,
    pub projection_digest: String,
}

impl ProviderReceiptProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_RECEIPT_PROJECTION_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.binding_id)
            || !valid_extension_identifier(&self.account_id)
            || !valid_extension_identifier(&self.operation)
            || !valid_prefixed_digest(&self.idempotency_key_digest)
            || normalize_provider_payload_hash(&self.payload_sha256).as_deref()
                != Some(self.payload_sha256.as_str())
            || !valid_extension_identifier(&self.provider_receipt_id)
            || !valid_extension_identifier(&self.source)
            || !valid_prefixed_digest(&self.result_digest)
            || !valid_prefixed_digest(&self.projection_digest)
        {
            return Err("provider_receipt_projection_invalid".to_owned());
        }
        if self.projection_digest != self.digest() {
            return Err("provider_receipt_projection_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "idempotency_key_digest": self.idempotency_key_digest,
            "payload_sha256": self.payload_sha256,
            "provider_receipt_id": self.provider_receipt_id,
            "outcome": self.outcome,
            "source": self.source,
            "result_digest": self.result_digest,
        }))
    }
}

/// Accept the historical unprefixed hash and the newer `sha256:` envelope spelling while keeping
/// the payload hash strictly hex and fixed-width.
pub fn provider_payload_hash_valid(value: &str) -> bool {
    is_sha256_hex(value) || value.strip_prefix("sha256:").is_some_and(is_sha256_hex)
}

/// Canonical lowercase, unprefixed spelling of a valid payload hash.  Every binding/projection
/// compare uses this form so the historical spelling variants cannot be used to evade binding.
pub fn normalize_provider_payload_hash(value: &str) -> Option<String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    is_sha256_hex(hex).then(|| hex.to_ascii_lowercase())
}

impl ProviderReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_RECEIPT_SCHEMA
            || !valid_extension_identifier(&self.connector_id)
            || !valid_extension_identifier(&self.binding_id)
            || !valid_extension_identifier(&self.account_id)
            || !valid_extension_identifier(&self.operation)
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.len() > 128
            || self.idempotency_key.chars().any(char::is_control)
            || !provider_payload_hash_valid(&self.final_payload_sha256)
            || !valid_extension_identifier(&self.provider_receipt_id)
            || !valid_extension_identifier(&self.source)
        {
            return Err("provider_receipt_header_invalid".to_owned());
        }
        if !self.result.is_object() {
            return Err("provider_receipt_result_invalid".to_owned());
        }
        if contains_forbidden_provider_result_key(&self.result) {
            return Err("provider_receipt_raw_response_forbidden".to_owned());
        }
        let header = serde_json::json!({
            "connector_id": self.connector_id,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "idempotency_key": self.idempotency_key,
            "provider_receipt_id": self.provider_receipt_id,
            "source": self.source,
        });
        crate::scan_secret_value(SecretScanChannel::Receipt, &header)
            .map_err(|_| "provider_receipt_raw_response_forbidden".to_owned())?;
        let result_bytes = serde_json::to_vec(&self.result)
            .map_err(|_| "provider_receipt_result_invalid".to_owned())?;
        if result_bytes.len() > PROVIDER_RECEIPT_MAX_RESULT_BYTES {
            return Err("provider_receipt_result_too_large".to_owned());
        }
        crate::scan_secret_value(SecretScanChannel::Receipt, &self.result)
            .map_err(|_| "provider_receipt_raw_response_forbidden".to_owned())?;
        Ok(())
    }

    /// Canonical payload hash without the optional `sha256:` envelope spelling.
    pub fn canonical_payload_sha256(&self) -> Result<String, String> {
        normalize_provider_payload_hash(&self.final_payload_sha256)
            .ok_or_else(|| "provider_receipt_header_invalid".to_owned())
    }

    /// Bind a receipt to the server-owned expectation for this attempt.  Cross-binding, account,
    /// operation, idempotency and payload evidence each fail with a distinct stable code.
    pub fn validate_for_binding(&self, binding: &ProviderReceiptBinding) -> Result<(), String> {
        self.validate()?;
        binding.validate()?;
        if self.connector_id != binding.connector_id
            || self.binding_id != binding.binding_id
            || self.account_id != binding.account_id
            || self.operation != binding.operation
        {
            return Err("provider_receipt_binding_mismatch".to_owned());
        }
        if json_digest(&serde_json::json!({"idempotency_key": self.idempotency_key}))
            != binding.idempotency_key_digest
        {
            return Err("provider_receipt_idempotency_mismatch".to_owned());
        }
        if let Some(expected) = &binding.payload_sha256 {
            if &self.canonical_payload_sha256()? != expected {
                return Err("provider_receipt_payload_mismatch".to_owned());
            }
        }
        Ok(())
    }

    /// Release the digest-only projection.  The raw result never leaves this method.
    pub fn projection(&self) -> Result<ProviderReceiptProjection, String> {
        self.validate()?;
        let mut projection = ProviderReceiptProjection {
            schema: PROVIDER_RECEIPT_PROJECTION_SCHEMA.to_owned(),
            connector_id: self.connector_id.clone(),
            binding_id: self.binding_id.clone(),
            account_id: self.account_id.clone(),
            operation: self.operation.clone(),
            idempotency_key_digest: json_digest(
                &serde_json::json!({"idempotency_key": self.idempotency_key}),
            ),
            payload_sha256: self.canonical_payload_sha256()?,
            provider_receipt_id: self.provider_receipt_id.clone(),
            outcome: self.outcome,
            source: self.source.clone(),
            result_digest: json_digest(&self.result),
            projection_digest: String::new(),
        };
        projection.projection_digest = projection.digest();
        projection.validate()?;
        Ok(projection)
    }
}

fn valid_prefixed_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(is_sha256_hex)
}

fn contains_forbidden_provider_result_key(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, nested)| {
            PROVIDER_RECEIPT_FORBIDDEN_RESULT_KEYS.contains(&key.as_str())
                || contains_forbidden_provider_result_key(nested)
        }),
        Value::Array(items) => items.iter().any(contains_forbidden_provider_result_key),
        _ => false,
    }
}

pub fn connector_bindings(
    events: &[crate::RuntimeEvent],
) -> Result<(u64, BTreeMap<String, ConnectorBindingSnapshot>), &'static str> {
    let mut version = 0;
    let mut bindings = BTreeMap::new();
    let mut dispatches: BTreeMap<(InvocationId, u32), ConnectorDispatchLifecycle> = BTreeMap::new();
    for event in events {
        if event.stream_version != Some(version + 1)
            || !matches!(
                event.kind.as_str(),
                "connector.binding"
                    | "connector.invoked"
                    | "connector.reconciled"
                    | CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND
                    | CONNECTOR_HEALTH_EVENT_KIND
                    | CONNECTOR_MCP_HANDSHAKE_EVENT_KIND
            )
        {
            return Err("connector_registry_event_invalid");
        }
        if event.kind == "connector.binding" {
            let state: ConnectorBindingSnapshot =
                serde_json::from_value(event.data["state"].clone())
                    .map_err(|_| "connector_registry_event_invalid")?;
            state.validate()?;
            if state.revision != version + 1 {
                return Err("connector_registry_event_invalid");
            }
            bindings.insert(state.binding.binding_id.clone(), state);
        } else if event.kind == "connector.invoked" {
            let receipt: ProviderReceipt = serde_json::from_value(event.data["receipt"].clone())
                .map_err(|_| "connector_registry_event_invalid")?;
            receipt
                .validate()
                .map_err(|_| "connector_registry_event_invalid")?;
            if let Some(raw_lifecycle) = event.data.get("dispatch_lifecycle") {
                let lifecycle: ConnectorDispatchLifecycle =
                    serde_json::from_value(raw_lifecycle.clone())
                        .map_err(|_| "connector_registry_event_invalid")?;
                lifecycle
                    .validate()
                    .map_err(|_| "connector_registry_event_invalid")?;
                let observation = EffectObservation::from_json(&event.data["effect_observation"])
                    .map_err(|_| "connector_registry_event_invalid")?;
                let identity = (lifecycle.invocation_id, lifecycle.attempt);
                if dispatches.get(&identity) != Some(&lifecycle)
                    || !lifecycle.stage.is_terminal()
                    || event.data["dispatch_lifecycle"]
                        != event.data["output"]["dispatch_lifecycle"]
                    || observation.invocation_id != lifecycle.invocation_id
                    || observation.attempt != lifecycle.attempt
                {
                    return Err("connector_registry_event_invalid");
                }
                let receipt_digest = json_digest(
                    &serde_json::to_value(&receipt)
                        .map_err(|_| "connector_registry_event_invalid")?,
                );
                observation
                    .validate_for_receipt(
                        &receipt,
                        &observation.owner_digest,
                        &observation.audience_digest,
                    )
                    .map_err(|_| "connector_registry_event_invalid")?;
                let result_digest = json_digest(&serde_json::json!({
                    "receipt": receipt,
                    "observation": observation,
                    "credential_evidence": event.data["credential_evidence"],
                }));
                if lifecycle.receipt_digest.as_deref() != Some(receipt_digest.as_str())
                    || lifecycle.observation_digest.as_deref()
                        != Some(observation.observation_digest.as_str())
                    || (lifecycle.stage == ConnectorDispatchStage::Unknown)
                        != (receipt.outcome == ProviderOutcome::Unknown)
                    || (lifecycle.stage == ConnectorDispatchStage::ResultCommitted
                        && lifecycle.result_digest.as_deref() != Some(result_digest.as_str()))
                {
                    return Err("connector_registry_event_invalid");
                }
            }
        } else if event.kind == "connector.reconciled" {
            let receipt: ProviderReceipt = serde_json::from_value(event.data["receipt"].clone())
                .map_err(|_| "connector_registry_event_invalid")?;
            receipt
                .validate()
                .map_err(|_| "connector_registry_event_invalid")?;
            let observation = EffectObservation::from_json(&event.data["effect_observation"])
                .map_err(|_| "connector_registry_event_invalid")?;
            observation
                .validate_for_receipt(
                    &receipt,
                    &observation.owner_digest,
                    &observation.audience_digest,
                )
                .map_err(|_| "connector_registry_event_invalid")?;
            if let Some(raw_case) = event.data.get("reconciliation_case") {
                let case: ConnectorReconciliationCase = serde_json::from_value(raw_case.clone())
                    .map_err(|_| "connector_registry_event_invalid")?;
                case.validate()
                    .map_err(|_| "connector_registry_event_invalid")?;
                if case.state != ConnectorReconciliationState::Reconciled
                    || case.evidence.as_ref().is_none_or(|evidence| {
                        evidence.receipt != receipt || evidence.observation != observation
                    })
                {
                    return Err("connector_registry_event_invalid");
                }
            }
        } else if event.kind == CONNECTOR_HEALTH_EVENT_KIND {
            let fact: ConnectorHealthFact = serde_json::from_value(event.data["health"].clone())
                .map_err(|_| "connector_registry_event_invalid")?;
            fact.validate()
                .map_err(|_| "connector_registry_event_invalid")?;
            // A health fact is scoped to one binding and one connector. A fact that reuses a
            // binding id but names another connector would otherwise let a projection relabel
            // the account health.
            if bindings
                .get(&fact.binding_id)
                .is_none_or(|binding| binding.definition.connector_id != fact.connector_id)
            {
                return Err("connector_registry_event_invalid");
            }
        } else if event.kind == CONNECTOR_MCP_HANDSHAKE_EVENT_KIND {
            let binding_id = event.data["binding_id"]
                .as_str()
                .ok_or("connector_registry_event_invalid")?;
            if !bindings.contains_key(binding_id) {
                return Err("connector_registry_event_invalid");
            }
            let handshake: crate::McpCapabilityHandshake =
                serde_json::from_value(event.data["handshake"].clone())
                    .map_err(|_| "connector_registry_event_invalid")?;
            handshake
                .validate()
                .map_err(|_| "connector_registry_event_invalid")?;
        } else if event.kind == CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND {
            let lifecycle: ConnectorDispatchLifecycle =
                serde_json::from_value(event.data["lifecycle"].clone())
                    .map_err(|_| "connector_registry_event_invalid")?;
            lifecycle
                .validate()
                .map_err(|_| "connector_registry_event_invalid")?;
            let identity = (lifecycle.invocation_id, lifecycle.attempt);
            match dispatches.get(&identity) {
                None if lifecycle.stage == ConnectorDispatchStage::Prepared => {}
                Some(previous) => {
                    let expected = previous
                        .advance(
                            lifecycle.stage,
                            lifecycle.receipt_digest.clone(),
                            lifecycle.observation_digest.clone(),
                            lifecycle.result_digest.clone(),
                            lifecycle.reason.clone(),
                        )
                        .map_err(|_| "connector_registry_event_invalid")?;
                    if expected != lifecycle {
                        return Err("connector_registry_event_invalid");
                    }
                }
                _ => return Err("connector_registry_event_invalid"),
            }
            dispatches.insert(identity, lifecycle);
        }
        version += 1;
    }
    Ok((version, bindings))
}

pub fn connector_invocation_risk(
    request: &crate::CapabilityRequest,
) -> Result<RiskLevel, &'static str> {
    connector_invocation_risk_for_arguments(&request.arguments)
}

pub(crate) fn connector_invocation_risk_for_arguments(
    arguments: &Value,
) -> Result<RiskLevel, &'static str> {
    let binding: ConnectorBindingSnapshot =
        serde_json::from_value(arguments["binding_snapshot"].clone())
            .map_err(|_| "connector_binding_snapshot_required")?;
    let operation = arguments["operation"]
        .as_str()
        .ok_or("connector_operation_required")?;
    let contract = binding.operation(operation)?;
    Ok(contract.connector_risk(operation).required_risk_level())
}
