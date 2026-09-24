//! Local Connector contracts. External transports are deliberately unsupported.

use crate::{
    connector_fixture_hash_valid, is_sha256_hex, valid_extension_identifier, valid_extension_path,
    RiskLevel, SecretRef,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CONNECTOR_MANAGE_OPERATION: &str = "connector.manage";
pub const CONNECTOR_INVOKE_OPERATION: &str = "connector.invoke";
/// Read-only binding health command. The command only authorizes an adapter probe; it never
/// grants an invocation permit or performs an external effect.
pub const CONNECTOR_HEALTH_OPERATION: &str = "connector.health";
pub const CONNECTOR_STREAM: &str = "connector_registry";
pub const CONNECTOR_HEALTH_EVENT_KIND: &str = "connector.health_checked";
pub const CONNECTOR_HEALTH_FACT_SCHEMA: &str = "kiana.connector-health-fact.v1";
pub const CONNECTOR_HEALTH_PROJECTION_SCHEMA: &str = "kiana.connector-health-projection.v1";
pub const CONNECTOR_HEALTH_MAX_LIMITATIONS: usize = 16;

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
pub fn classify_connector_health_error(error_code: &str) -> ConnectorHealthStatus {
    match error_code {
        "credential_invalid" | "invalid_credential" | "unauthorized" | "invalid_token" => {
            ConnectorHealthStatus::CredentialInvalid
        }
        "scope_insufficient" | "missing_scope" | "insufficient_scope" => {
            ConnectorHealthStatus::ScopeInsufficient
        }
        "endpoint_unreachable" | "dns_failed" | "connection_refused" | "timeout" => {
            ConnectorHealthStatus::EndpointUnreachable
        }
        "unsupported" | "transport_unsupported" => ConnectorHealthStatus::Unsupported,
        _ => ConnectorHealthStatus::ProviderError,
    }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorDefinition {
    pub schema: String,
    pub connector_id: String,
    pub version: String,
    pub provider_id: String,
    /// This release accepts only local_fixture and never sends external requests.
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
        if definition.adapter != "local_fixture" || definition.data_processing != "local_only" {
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

pub fn connector_bindings(
    events: &[crate::RuntimeEvent],
) -> Result<(u64, BTreeMap<String, ConnectorBindingSnapshot>), &'static str> {
    let mut version = 0;
    let mut bindings = BTreeMap::new();
    for event in events {
        if event.stream_version != Some(version + 1)
            || !matches!(
                event.kind.as_str(),
                "connector.binding"
                    | "connector.invoked"
                    | "connector.reconciled"
                    | CONNECTOR_HEALTH_EVENT_KIND
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
        } else if event.kind == CONNECTOR_HEALTH_EVENT_KIND {
            let fact: ConnectorHealthFact = serde_json::from_value(event.data["health"].clone())
                .map_err(|_| "connector_registry_event_invalid")?;
            fact.validate()
                .map_err(|_| "connector_registry_event_invalid")?;
            if !bindings.contains_key(&fact.binding_id) {
                return Err("connector_registry_event_invalid");
            }
        }
        version += 1;
    }
    Ok((version, bindings))
}

pub fn connector_invocation_risk(
    request: &crate::CapabilityRequest,
) -> Result<RiskLevel, &'static str> {
    let binding: ConnectorBindingSnapshot =
        serde_json::from_value(request.arguments["binding_snapshot"].clone())
            .map_err(|_| "connector_binding_snapshot_required")?;
    let operation = request.arguments["operation"]
        .as_str()
        .ok_or("connector_operation_required")?;
    Ok(binding.operation(operation)?.risk())
}
