//! Local Connector contracts. External transports are deliberately unsupported.

use crate::{is_sha256_hex, valid_extension_identifier, valid_extension_path, RiskLevel};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CONNECTOR_MANAGE_OPERATION: &str = "connector.manage";
pub const CONNECTOR_INVOKE_OPERATION: &str = "connector.invoke";
pub const CONNECTOR_STREAM: &str = "connector_registry";

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
            || !is_sha256_hex(&binding.fixture_sha256)
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
                "connector.binding" | "connector.invoked" | "connector.reconciled"
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
