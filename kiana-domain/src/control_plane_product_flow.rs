//! CP-30 product-flow and evidence closeout contract.
//!
//! The contract is a CI fixture boundary over the existing DaemonHost/ControlPlane path. It
//! compares the read-only, approval/write and cancel/recovery flows across four surfaces without
//! creating a UI loop, a fake receipt, a second Broker or a second source of truth.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CONTROL_PLANE_PRODUCT_FLOW_SCHEMA: &str = "kiana.control-plane-product-flow.v1";
pub const CONTROL_PLANE_PRODUCT_BUNDLE_SCHEMA: &str = "kiana.control-plane-product-bundle.v1";

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cp30FlowKind {
    ReadOnly,
    ApprovalWrite,
    CancelRecovery,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cp30FlowStatus {
    Completed,
    AwaitingApproval,
    Cancelled,
    ResultUnknown,
    Reconciled,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cp30Surface {
    Cli,
    Workbench,
    Web,
    Desktop,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cp30SurfaceReceipt {
    pub schema: String,
    pub surface: Cp30Surface,
    pub flow_id: String,
    pub authority_digest: String,
    pub event_cursor: u64,
    pub status: Cp30FlowStatus,
    pub receipt_digest: Option<String>,
    pub digest: String,
}

impl Cp30SurfaceReceipt {
    pub fn new(
        surface: Cp30Surface,
        flow_id: impl Into<String>,
        authority_digest: impl Into<String>,
        event_cursor: u64,
        status: Cp30FlowStatus,
        receipt_digest: Option<String>,
    ) -> Self {
        let mut receipt = Self {
            schema: CONTROL_PLANE_PRODUCT_FLOW_SCHEMA.to_owned(),
            surface,
            flow_id: flow_id.into(),
            authority_digest: authority_digest.into(),
            event_cursor,
            status,
            receipt_digest,
            digest: String::new(),
        };
        receipt.digest = receipt.canonical_digest();
        receipt
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_PRODUCT_FLOW_SCHEMA || self.event_cursor == 0 {
            return Err("cp30_surface_receipt_header_invalid".to_owned());
        }
        required(&self.flow_id, "cp30_surface_flow_required")?;
        digest(
            &self.authority_digest,
            "cp30_surface_authority_digest_invalid",
        )?;
        if let Some(receipt) = &self.receipt_digest {
            digest(receipt, "cp30_surface_receipt_digest_invalid")?;
        }
        digest(&self.digest, "cp30_surface_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp30_surface_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "surface": self.surface,
            "flow_id": self.flow_id,
            "authority_digest": self.authority_digest,
            "event_cursor": self.event_cursor,
            "status": self.status,
            "receipt_digest": self.receipt_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cp30ProductFlowEvidence {
    pub schema: String,
    pub flow_id: String,
    pub kind: Cp30FlowKind,
    pub status: Cp30FlowStatus,
    pub authority_digest: String,
    pub event_cursor: u64,
    pub model_calls: u32,
    pub broker_calls: u32,
    pub effect_count: u32,
    pub approval_consumed: bool,
    pub cache_removed: bool,
    pub restored_from_facts: bool,
    pub surfaces: Vec<Cp30SurfaceReceipt>,
    pub receipt_digest: Option<String>,
    pub limitations: Vec<String>,
    pub digest: String,
}

impl Cp30ProductFlowEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        flow_id: impl Into<String>,
        kind: Cp30FlowKind,
        status: Cp30FlowStatus,
        authority_digest: impl Into<String>,
        event_cursor: u64,
        model_calls: u32,
        broker_calls: u32,
        effect_count: u32,
        approval_consumed: bool,
        cache_removed: bool,
        restored_from_facts: bool,
        surfaces: Vec<Cp30SurfaceReceipt>,
        receipt_digest: Option<String>,
        limitations: Vec<String>,
    ) -> Self {
        let mut evidence = Self {
            schema: CONTROL_PLANE_PRODUCT_FLOW_SCHEMA.to_owned(),
            flow_id: flow_id.into(),
            kind,
            status,
            authority_digest: authority_digest.into(),
            event_cursor,
            model_calls,
            broker_calls,
            effect_count,
            approval_consumed,
            cache_removed,
            restored_from_facts,
            surfaces,
            receipt_digest,
            limitations,
            digest: String::new(),
        };
        evidence.digest = evidence.canonical_digest();
        evidence
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_PRODUCT_FLOW_SCHEMA
            || self.event_cursor == 0
            || self.surfaces.len() != 4
            || self.limitations.len() > 32
        {
            return Err("cp30_product_flow_header_invalid".to_owned());
        }
        required(&self.flow_id, "cp30_flow_id_required")?;
        digest(&self.authority_digest, "cp30_authority_digest_invalid")?;
        if let Some(receipt) = &self.receipt_digest {
            digest(receipt, "cp30_receipt_digest_invalid")?;
        }
        for limitation in &self.limitations {
            required(limitation, "cp30_limitation_invalid")?;
        }
        let mut surfaces = BTreeSet::new();
        for receipt in &self.surfaces {
            receipt.validate()?;
            if receipt.flow_id != self.flow_id
                || receipt.authority_digest != self.authority_digest
                || receipt.event_cursor != self.event_cursor
                || receipt.status != self.status
                || receipt.receipt_digest != self.receipt_digest
                || !surfaces.insert(receipt.surface)
            {
                return Err("cp30_surface_parity_mismatch".to_owned());
            }
        }
        let expected = [
            Cp30Surface::Cli,
            Cp30Surface::Workbench,
            Cp30Surface::Web,
            Cp30Surface::Desktop,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if surfaces != expected {
            return Err("cp30_surface_set_incomplete".to_owned());
        }
        match self.kind {
            Cp30FlowKind::ReadOnly => {
                if self.status != Cp30FlowStatus::Completed
                    || self.effect_count != 0
                    || self.broker_calls != 0
                    || self.approval_consumed
                    || self.model_calls == 0
                {
                    return Err("cp30_read_only_flow_effect_or_approval_invalid".to_owned());
                }
            }
            Cp30FlowKind::ApprovalWrite => {
                if self.status == Cp30FlowStatus::Completed
                    && (!self.approval_consumed
                        || self.broker_calls != 1
                        || self.effect_count != 1
                        || self.receipt_digest.is_none())
                {
                    return Err("cp30_approval_write_requires_consumed_receipt".to_owned());
                }
                if self.status == Cp30FlowStatus::AwaitingApproval
                    && (self.approval_consumed || self.broker_calls != 0 || self.effect_count != 0)
                {
                    return Err("cp30_pending_approval_has_effect".to_owned());
                }
            }
            Cp30FlowKind::CancelRecovery => {
                if !matches!(
                    self.status,
                    Cp30FlowStatus::Cancelled
                        | Cp30FlowStatus::ResultUnknown
                        | Cp30FlowStatus::Reconciled
                ) || !self.cache_removed
                    || !self.restored_from_facts
                {
                    return Err("cp30_cancel_recovery_evidence_incomplete".to_owned());
                }
                if self.status == Cp30FlowStatus::Cancelled && self.effect_count > 0 {
                    return Err("cp30_cancelled_effect_count_invalid".to_owned());
                }
            }
        }
        digest(&self.digest, "cp30_flow_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp30_flow_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "flow_id": self.flow_id,
            "kind": self.kind,
            "status": self.status,
            "authority_digest": self.authority_digest,
            "event_cursor": self.event_cursor,
            "model_calls": self.model_calls,
            "broker_calls": self.broker_calls,
            "effect_count": self.effect_count,
            "approval_consumed": self.approval_consumed,
            "cache_removed": self.cache_removed,
            "restored_from_facts": self.restored_from_facts,
            "surfaces": self.surfaces,
            "receipt_digest": self.receipt_digest,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cp30ProductFlowBundle {
    pub schema: String,
    pub flows: Vec<Cp30ProductFlowEvidence>,
    pub digest: String,
}

impl Cp30ProductFlowBundle {
    pub fn new(flows: Vec<Cp30ProductFlowEvidence>) -> Self {
        let mut bundle = Self {
            schema: CONTROL_PLANE_PRODUCT_BUNDLE_SCHEMA.to_owned(),
            flows,
            digest: String::new(),
        };
        bundle.digest = bundle.canonical_digest();
        bundle
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_PRODUCT_BUNDLE_SCHEMA || self.flows.len() != 3 {
            return Err("cp30_product_bundle_header_invalid".to_owned());
        }
        digest(&self.digest, "cp30_bundle_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp30_bundle_digest_mismatch".to_owned());
        }
        let mut kinds = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for flow in &self.flows {
            flow.validate()?;
            if !kinds.insert(flow.kind) || !ids.insert(flow.flow_id.clone()) {
                return Err("cp30_product_flow_duplicate".to_owned());
            }
        }
        let expected = [
            Cp30FlowKind::ReadOnly,
            Cp30FlowKind::ApprovalWrite,
            Cp30FlowKind::CancelRecovery,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if kinds != expected {
            return Err("cp30_product_flow_set_incomplete".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "flows": self.flows,
        }))
    }
}
