//! Strict Connector operation input/output, risk and retry contract.

use crate::{json_digest, ConnectorEffect, RiskLevel};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CONNECTOR_OPERATION_CONTRACT_SCHEMA: &str = "kiana.connector-operation-contract.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorIdempotencyMode {
    Required,
    Optional,
    Forbidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRetryMode {
    Never,
    KnownNoEffect,
    DeclaredIdempotent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorOperationContract {
    pub schema: String,
    pub operation_id: String,
    pub effect: ConnectorEffect,
    pub risk: RiskLevel,
    pub input_schema_digest: String,
    pub output_schema_digest: String,
    pub required_scopes: BTreeSet<String>,
    pub data_classes: BTreeSet<String>,
    pub max_input_bytes: u64,
    pub timeout_ms: u64,
    pub retry: ConnectorRetryMode,
    pub idempotency: ConnectorIdempotencyMode,
    pub contract_digest: String,
}

impl ConnectorOperationContract {
    /// Map the versioned contract into the connector R0--R4 policy vocabulary.  The server-owned
    /// declared risk remains an input to the conservative mapping; callers cannot lower it by
    /// changing request arguments.
    pub fn connector_risk(&self) -> crate::ConnectorOperationRisk {
        crate::connector_operation_risk_with_declared(&self.operation_id, self.effect, self.risk)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_OPERATION_CONTRACT_SCHEMA
            || self.operation_id.trim().is_empty()
            || self.operation_id.len() > 256
            || self
                .required_scopes
                .iter()
                .any(|scope| scope.trim().is_empty())
            || self
                .data_classes
                .iter()
                .any(|class| class.trim().is_empty())
            || self.max_input_bytes == 0
            || self.max_input_bytes > 16 * 1024 * 1024
            || self.timeout_ms == 0
            || self.timeout_ms > 60 * 60 * 1000
        {
            return Err("connector_operation_contract_header_invalid".to_owned());
        }
        if self.effect == ConnectorEffect::ReadOnly
            && !matches!(self.risk, RiskLevel::ReadOnly | RiskLevel::LocalWrite)
        {
            return Err("connector_operation_risk_downgrade_or_mismatch".to_owned());
        }
        if self.effect == ConnectorEffect::ReadOnly
            && self.risk == RiskLevel::LocalWrite
            && self.connector_risk() != crate::ConnectorOperationRisk::R2DataGrant
        {
            return Err("connector_operation_data_grant_risk_invalid".to_owned());
        }
        if self.effect == ConnectorEffect::Write
            && !matches!(
                self.risk,
                RiskLevel::ExternalSideEffect | RiskLevel::Critical
            )
        {
            return Err("connector_operation_write_risk_invalid".to_owned());
        }
        if self.idempotency == ConnectorIdempotencyMode::Required
            && self.retry == ConnectorRetryMode::DeclaredIdempotent
        {
            return Err("connector_operation_retry_idempotency_conflict".to_owned());
        }
        for (value, field) in [
            (
                &self.input_schema_digest,
                "connector_operation_input_schema_digest",
            ),
            (
                &self.output_schema_digest,
                "connector_operation_output_schema_digest",
            ),
            (&self.contract_digest, "connector_operation_contract_digest"),
        ] {
            let Some(hex) = value.strip_prefix("sha256:") else {
                return Err(format!("{field}_invalid"));
            };
            if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.contract_digest != self.digest() {
            return Err("connector_operation_contract_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "operation_id": self.operation_id,
            "effect": self.effect,
            "risk": self.risk,
            "input_schema_digest": self.input_schema_digest,
            "output_schema_digest": self.output_schema_digest,
            "required_scopes": self.required_scopes,
            "data_classes": self.data_classes,
            "max_input_bytes": self.max_input_bytes,
            "timeout_ms": self.timeout_ms,
            "retry": self.retry,
            "idempotency": self.idempotency,
        }))
    }
}
