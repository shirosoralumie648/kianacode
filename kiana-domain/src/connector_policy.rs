//! Server-owned connector operation risk to policy/gate/approval mapping.
//!
//! Connector operations use the integration-specific R0--R4 vocabulary while the execution
//! spine continues to use [`RiskLevel`].  This module is the small, deterministic bridge between
//! the two.  It derives risk from the registered operation name/effect; a caller supplied risk
//! label is only checked for downgrade and never used to select a safer policy.

use crate::{json_digest, ConnectorEffect, ConnectorOperation, RiskLevel};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const CONNECTOR_POLICY_SCHEMA: &str = "kiana.connector-policy.v1";
pub const CONNECTOR_DATA_GRANT_SCHEMA: &str = "kiana.connector-data-grant.v1";
pub const CONNECTOR_ONCE_APPROVAL_SCHEMA: &str = "kiana.connector-once-approval.v1";

/// Connector-specific risk classes.  R0 and R1 are both read-only at the execution spine;
/// R2 carries a data-processing grant, R3 carries one exact final-payload approval, and R4 is
/// denied by default.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorOperationRisk {
    R0ReadOnly,
    R1ReadOnly,
    R2DataGrant,
    R3OnceApproval,
    R4DefaultDeny,
}

impl ConnectorOperationRisk {
    pub const fn required_risk_level(self) -> RiskLevel {
        match self {
            Self::R0ReadOnly | Self::R1ReadOnly => RiskLevel::ReadOnly,
            Self::R2DataGrant => RiskLevel::LocalWrite,
            Self::R3OnceApproval => RiskLevel::ExternalSideEffect,
            Self::R4DefaultDeny => RiskLevel::Critical,
        }
    }

    pub const fn policy_requirement(self) -> ConnectorPolicyRequirement {
        match self {
            Self::R0ReadOnly | Self::R1ReadOnly => ConnectorPolicyRequirement::ReadOnly,
            Self::R2DataGrant => ConnectorPolicyRequirement::DataGrant,
            Self::R3OnceApproval => ConnectorPolicyRequirement::OnceApproval,
            Self::R4DefaultDeny => ConnectorPolicyRequirement::DefaultDeny,
        }
    }
}

/// The policy/gate/approval action selected by a server-owned operation contract.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorPolicyRequirement {
    ReadOnly,
    DataGrant,
    OnceApproval,
    DefaultDeny,
}

/// Derive the connector risk from the immutable operation contract.  The operation name is part
/// of that contract, not caller input at this point, so conservative R2/R4 names cannot be
/// relabelled as a read-only call.  Unknown read operations remain R1; unknown writes remain R3.
pub fn connector_operation_risk(
    operation_id: &str,
    effect: ConnectorEffect,
) -> ConnectorOperationRisk {
    let operation = operation_id.to_ascii_lowercase();
    if contains_any(
        &operation,
        &[
            "delete",
            "payment",
            "refund",
            "purchase",
            "checkout",
            "production",
            "release",
            "firmware",
            "pairing",
        ],
    ) {
        return ConnectorOperationRisk::R4DefaultDeny;
    }
    if contains_any(
        &operation,
        &[
            "export",
            "batch",
            "share",
            "artifact",
            "cross_boundary",
            "cross-boundary",
        ],
    ) {
        return ConnectorOperationRisk::R2DataGrant;
    }
    if effect == ConnectorEffect::ReadOnly {
        if contains_any(&operation, &["list", "inspect", "schema", "health"]) {
            ConnectorOperationRisk::R0ReadOnly
        } else {
            ConnectorOperationRisk::R1ReadOnly
        }
    } else {
        ConnectorOperationRisk::R3OnceApproval
    }
}

/// Variant used by the strict INT-03 operation contract, whose server-owned `RiskLevel` carries
/// information that is not present on the older binding projection.  A Critical contract always
/// remains R4; LocalWrite is the conservative R2 data-grant class.
pub fn connector_operation_risk_with_declared(
    operation_id: &str,
    effect: ConnectorEffect,
    declared: RiskLevel,
) -> ConnectorOperationRisk {
    match declared {
        RiskLevel::Critical => ConnectorOperationRisk::R4DefaultDeny,
        RiskLevel::LocalWrite => ConnectorOperationRisk::R2DataGrant,
        RiskLevel::ExternalSideEffect => {
            let inferred = connector_operation_risk(operation_id, effect);
            if inferred == ConnectorOperationRisk::R4DefaultDeny {
                inferred
            } else {
                ConnectorOperationRisk::R3OnceApproval
            }
        }
        RiskLevel::ReadOnly => connector_operation_risk(operation_id, effect),
    }
}

impl ConnectorOperation {
    /// Resolve the operation class from the server-owned binding snapshot.
    pub fn connector_risk(&self, operation_id: &str) -> ConnectorOperationRisk {
        connector_operation_risk(operation_id, self.effect)
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| value == *needle || value.contains(needle))
}

/// A cross-boundary data grant required by R2.  The grant is a narrowing, server-issued object;
/// it is never inferred from a payload or accepted from a wire authority field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorDataGrant {
    pub schema: String,
    pub grant_id: String,
    pub binding_id: String,
    pub owner_id: String,
    pub project_root: String,
    pub data_classes: BTreeSet<String>,
    pub data_epoch: u64,
    pub authority_epoch: u64,
    pub policy_epoch: u64,
    pub expires_at_unix_ms: u64,
    pub revoked: bool,
    pub grant_digest: String,
}

impl ConnectorDataGrant {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        grant_id: impl Into<String>,
        binding_id: impl Into<String>,
        owner_id: impl Into<String>,
        project_root: impl Into<String>,
        data_classes: BTreeSet<String>,
        data_epoch: u64,
        authority_epoch: u64,
        policy_epoch: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut grant = Self {
            schema: CONNECTOR_DATA_GRANT_SCHEMA.to_owned(),
            grant_id: grant_id.into(),
            binding_id: binding_id.into(),
            owner_id: owner_id.into(),
            project_root: project_root.into(),
            data_classes,
            data_epoch,
            authority_epoch,
            policy_epoch,
            expires_at_unix_ms,
            revoked: false,
            grant_digest: String::new(),
        };
        grant.grant_digest = grant.digest();
        grant.validate()?;
        Ok(grant)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_DATA_GRANT_SCHEMA
            || !nonempty(&self.grant_id)
            || !nonempty(&self.binding_id)
            || !nonempty(&self.owner_id)
            || !nonempty(&self.project_root)
            || self.data_classes.is_empty()
            || self.data_classes.iter().any(|class| !nonempty(class))
            || self.data_epoch == 0
            || self.authority_epoch == 0
            || self.policy_epoch == 0
            || self.expires_at_unix_ms == 0
            || !valid_digest(&self.grant_digest)
        {
            return Err("connector_data_grant_invalid".to_owned());
        }
        if self.grant_digest != self.digest() {
            return Err("connector_data_grant_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn allows(
        &self,
        binding_id: &str,
        owner_id: &str,
        project_root: &str,
        required_classes: &BTreeSet<String>,
        data_epoch: u64,
        authority_epoch: u64,
        policy_epoch: u64,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.revoked {
            return Err("connector_data_grant_revoked".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("connector_data_grant_expired".to_owned());
        }
        if self.binding_id != binding_id
            || self.owner_id != owner_id
            || self.project_root != project_root
        {
            return Err("connector_data_grant_authority_mismatch".to_owned());
        }
        if self.data_epoch != data_epoch {
            return Err("connector_data_grant_data_epoch_mismatch".to_owned());
        }
        if self.authority_epoch != authority_epoch {
            return Err("connector_data_grant_authority_epoch_mismatch".to_owned());
        }
        if self.policy_epoch != policy_epoch {
            return Err("connector_data_grant_policy_epoch_mismatch".to_owned());
        }
        if !required_classes.is_subset(&self.data_classes) {
            return Err("connector_data_grant_scope_insufficient".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "grant_id": self.grant_id,
            "binding_id": self.binding_id,
            "owner_id": self.owner_id,
            "project_root": self.project_root,
            "data_classes": self.data_classes,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "policy_epoch": self.policy_epoch,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "revoked": self.revoked,
        }))
    }
}

/// A one-shot approval bound to the final canonical payload.  It is intentionally separate from
/// a display preview: the ControlPlane approval journal remains the authority that consumes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorOnceApproval {
    pub schema: String,
    pub approval_id: String,
    pub binding_id: String,
    pub operation: String,
    pub payload_digest: String,
    pub authority_epoch: u64,
    pub policy_epoch: u64,
    pub expires_at_unix_ms: u64,
    pub consumed: bool,
    pub revoked: bool,
    pub approval_digest: String,
}

impl ConnectorOnceApproval {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        approval_id: impl Into<String>,
        binding_id: impl Into<String>,
        operation: impl Into<String>,
        payload_digest: impl Into<String>,
        authority_epoch: u64,
        policy_epoch: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut approval = Self {
            schema: CONNECTOR_ONCE_APPROVAL_SCHEMA.to_owned(),
            approval_id: approval_id.into(),
            binding_id: binding_id.into(),
            operation: operation.into(),
            payload_digest: payload_digest.into(),
            authority_epoch,
            policy_epoch,
            expires_at_unix_ms,
            consumed: false,
            revoked: false,
            approval_digest: String::new(),
        };
        approval.approval_digest = approval.digest();
        approval.validate()?;
        Ok(approval)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_ONCE_APPROVAL_SCHEMA
            || !nonempty(&self.approval_id)
            || !nonempty(&self.binding_id)
            || !nonempty(&self.operation)
            || !valid_digest(&self.payload_digest)
            || self.authority_epoch == 0
            || self.policy_epoch == 0
            || self.expires_at_unix_ms == 0
            || !valid_digest(&self.approval_digest)
        {
            return Err("connector_once_approval_invalid".to_owned());
        }
        if self.approval_digest != self.digest() {
            return Err("connector_once_approval_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn allows(
        &self,
        binding_id: &str,
        operation: &str,
        final_payload_digest: &str,
        authority_epoch: u64,
        policy_epoch: u64,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.revoked {
            return Err("connector_once_approval_revoked".to_owned());
        }
        if self.consumed {
            return Err("connector_once_approval_consumed".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("connector_once_approval_expired".to_owned());
        }
        if self.binding_id != binding_id || self.operation != operation {
            return Err("connector_once_approval_authority_mismatch".to_owned());
        }
        if self.payload_digest != final_payload_digest {
            return Err("connector_once_approval_payload_digest_mismatch".to_owned());
        }
        if self.authority_epoch != authority_epoch {
            return Err("connector_once_approval_authority_epoch_mismatch".to_owned());
        }
        if self.policy_epoch != policy_epoch {
            return Err("connector_once_approval_policy_epoch_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "approval_id": self.approval_id,
            "binding_id": self.binding_id,
            "operation": self.operation,
            "payload_digest": self.payload_digest,
            "authority_epoch": self.authority_epoch,
            "policy_epoch": self.policy_epoch,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "consumed": self.consumed,
            "revoked": self.revoked,
        }))
    }
}

/// Server-owned context passed to the pure connector admission mapping.
#[derive(Clone, Debug)]
pub struct ConnectorAdmissionInput<'a> {
    pub operation: &'a str,
    pub effect: ConnectorEffect,
    pub required_data_classes: &'a BTreeSet<String>,
    pub declared_risk: RiskLevel,
    pub binding_id: &'a str,
    pub owner_id: &'a str,
    pub project_root: &'a str,
    pub binding_active: bool,
    pub binding_expires_at_unix_ms: u64,
    pub binding_authority_epoch: u64,
    pub binding_policy_epoch: u64,
    pub authority_epoch: u64,
    pub policy_epoch: u64,
    pub data_epoch: u64,
    pub now_unix_ms: u64,
    pub payload: Option<&'a Value>,
    pub final_payload_digest: Option<String>,
    pub data_grant: Option<&'a ConnectorDataGrant>,
    pub once_approval: Option<&'a ConnectorOnceApproval>,
}

impl<'a> ConnectorAdmissionInput<'a> {
    pub fn new(
        operation: &'a str,
        effect: ConnectorEffect,
        required_data_classes: &'a BTreeSet<String>,
        declared_risk: RiskLevel,
        binding_id: &'a str,
        owner_id: &'a str,
        project_root: &'a str,
        payload: Option<&'a Value>,
    ) -> Self {
        Self {
            operation,
            effect,
            required_data_classes,
            declared_risk,
            binding_id,
            owner_id,
            project_root,
            binding_active: true,
            binding_expires_at_unix_ms: u64::MAX,
            binding_authority_epoch: 1,
            binding_policy_epoch: 1,
            authority_epoch: 1,
            policy_epoch: 1,
            data_epoch: 1,
            now_unix_ms: 1,
            payload,
            final_payload_digest: None,
            data_grant: None,
            once_approval: None,
        }
    }

    pub fn with_epochs(mut self, authority_epoch: u64, policy_epoch: u64, data_epoch: u64) -> Self {
        self.authority_epoch = authority_epoch;
        self.policy_epoch = policy_epoch;
        self.data_epoch = data_epoch;
        self.binding_authority_epoch = authority_epoch;
        self.binding_policy_epoch = policy_epoch;
        self
    }

    pub fn with_binding_expiry(mut self, expires_at_unix_ms: u64) -> Self {
        self.binding_expires_at_unix_ms = expires_at_unix_ms;
        self
    }

    pub fn with_now(mut self, now_unix_ms: u64) -> Self {
        self.now_unix_ms = now_unix_ms;
        self
    }

    pub fn with_final_payload_digest(mut self, digest: Option<&str>) -> Self {
        self.final_payload_digest = digest.map(str::to_owned);
        self
    }

    pub fn with_data_grant(mut self, grant: Option<&'a ConnectorDataGrant>) -> Self {
        self.data_grant = grant;
        self
    }

    pub fn with_once_approval(mut self, approval: Option<&'a ConnectorOnceApproval>) -> Self {
        self.once_approval = approval;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ConnectorAdmission {
    Allowed {
        risk: ConnectorOperationRisk,
        final_payload_digest: Option<String>,
        approval_id: Option<String>,
        broker_calls: u32,
    },
    AwaitingApproval {
        risk: ConnectorOperationRisk,
        reason: String,
        final_payload_digest: String,
        broker_calls: u32,
    },
    Denied {
        risk: ConnectorOperationRisk,
        reason: String,
        broker_calls: u32,
    },
}

impl ConnectorAdmission {
    pub const fn broker_calls(&self) -> u32 {
        match self {
            Self::Allowed { broker_calls, .. }
            | Self::AwaitingApproval { broker_calls, .. }
            | Self::Denied { broker_calls, .. } => *broker_calls,
        }
    }
}

/// Apply binding, epoch, data-grant, final-payload and once-approval checks in a fixed order.
/// Every non-allowed result carries `broker_calls = 0`; this is the source-level deny contract
/// consumed by the policy/core fixtures.
pub fn evaluate_connector_admission(input: &ConnectorAdmissionInput<'_>) -> ConnectorAdmission {
    let risk = connector_operation_risk(input.operation, input.effect);
    let denied = |reason: &str| ConnectorAdmission::Denied {
        risk,
        reason: reason.to_owned(),
        broker_calls: 0,
    };
    if input.operation.trim().is_empty()
        || input.binding_id.trim().is_empty()
        || input.owner_id.trim().is_empty()
        || input.project_root.trim().is_empty()
        || input.authority_epoch == 0
        || input.policy_epoch == 0
        || input.data_epoch == 0
        || input.now_unix_ms == 0
    {
        return denied("connector_admission_context_invalid");
    }
    if !input.binding_active {
        return denied("connector_binding_revoked");
    }
    if input.now_unix_ms >= input.binding_expires_at_unix_ms {
        return denied("connector_binding_expired");
    }
    if input.binding_authority_epoch != input.authority_epoch {
        return denied("connector_authority_epoch_mismatch");
    }
    if input.binding_policy_epoch != input.policy_epoch {
        return denied("connector_policy_epoch_mismatch");
    }
    if risk_rank(input.declared_risk) < risk_rank(risk.required_risk_level()) {
        return denied(match risk {
            ConnectorOperationRisk::R3OnceApproval => "connector_final_payload_approval_required",
            _ => "connector_risk_downgrade",
        });
    }
    if risk == ConnectorOperationRisk::R4DefaultDeny {
        return denied("connector_r4_default_denied");
    }

    let payload_digest = input.payload.map(crate::connector_payload_sha256);
    if let Some(expected) = payload_digest.as_deref() {
        match input.final_payload_digest.as_deref() {
            Some(actual) if actual == expected => {}
            Some(_) => return denied("connector_final_payload_digest_mismatch"),
            None if risk == ConnectorOperationRisk::R3OnceApproval => {
                return denied("connector_final_payload_digest_required")
            }
            None => {}
        }
    }
    match risk.policy_requirement() {
        ConnectorPolicyRequirement::ReadOnly => ConnectorAdmission::Allowed {
            risk,
            final_payload_digest: payload_digest,
            approval_id: None,
            broker_calls: 1,
        },
        ConnectorPolicyRequirement::DataGrant => {
            let Some(grant) = input.data_grant else {
                return denied("connector_data_grant_required");
            };
            if let Err(reason) = grant.allows(
                input.binding_id,
                input.owner_id,
                input.project_root,
                input.required_data_classes,
                input.data_epoch,
                input.authority_epoch,
                input.policy_epoch,
                input.now_unix_ms,
            ) {
                return denied(&reason);
            }
            ConnectorAdmission::Allowed {
                risk,
                final_payload_digest: payload_digest,
                approval_id: None,
                broker_calls: 1,
            }
        }
        ConnectorPolicyRequirement::OnceApproval => {
            let final_payload_digest = payload_digest
                .or_else(|| input.final_payload_digest.clone())
                .unwrap_or_default();
            let Some(approval) = input.once_approval else {
                return ConnectorAdmission::AwaitingApproval {
                    risk,
                    reason: "connector_final_payload_approval_required".to_owned(),
                    final_payload_digest,
                    broker_calls: 0,
                };
            };
            if let Err(reason) = approval.allows(
                input.binding_id,
                input.operation,
                &final_payload_digest,
                input.authority_epoch,
                input.policy_epoch,
                input.now_unix_ms,
            ) {
                return denied(&reason);
            }
            ConnectorAdmission::Allowed {
                risk,
                final_payload_digest: Some(final_payload_digest),
                approval_id: Some(approval.approval_id.clone()),
                broker_calls: 1,
            }
        }
        ConnectorPolicyRequirement::DefaultDeny => denied("connector_r4_default_denied"),
    }
}

fn risk_rank(risk: RiskLevel) -> u8 {
    match risk {
        RiskLevel::ReadOnly => 0,
        RiskLevel::LocalWrite => 1,
        RiskLevel::ExternalSideEffect => 2,
        RiskLevel::Critical => 3,
    }
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
