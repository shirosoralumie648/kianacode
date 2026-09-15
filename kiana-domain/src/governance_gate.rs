//! Cross-entrypoint CompanyOS governance projection.
//!
//! Runtime completion, business acceptance, delivery confirmation and project close are distinct
//! facts. This snapshot makes their references comparable without promoting one layer into
//! another layer's authority.

use crate::{
    canonical_journal_bytes, json_digest, AcceptanceStatus, DeliveryStatus, EventId,
    ExecutionStatus, ProjectStatus, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_GOVERNANCE_SCHEMA: &str = "kiana.company-governance.v1";
pub const COMPANY_GOVERNANCE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_GOVERNANCE_RUNS: usize = 256;
pub const MAX_GOVERNANCE_REFS: usize = 256;
pub const MAX_GOVERNANCE_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceStatus {
    InProgress,
    Accepted,
    Closed,
    Unknown,
}

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyGovernanceSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_id: String,
    pub project_status: ProjectStatus,
    pub runtime_statuses: BTreeMap<String, ExecutionStatus>,
    #[serde(default)]
    pub acceptance_status: Option<AcceptanceStatus>,
    #[serde(default)]
    pub review_id: Option<String>,
    #[serde(default)]
    pub delivery_status: Option<DeliveryStatus>,
    #[serde(default)]
    pub closing_receipt_id: Option<String>,
    #[serde(default)]
    pub outcome_ids: Vec<String>,
    pub source_event_ids: Vec<EventId>,
    pub status: GovernanceStatus,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub snapshot_digest: String,
}

impl CompanyGovernanceSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: impl Into<String>,
        project_status: ProjectStatus,
        runtime_statuses: BTreeMap<String, ExecutionStatus>,
        acceptance_status: Option<AcceptanceStatus>,
        review_id: Option<String>,
        delivery_status: Option<DeliveryStatus>,
        closing_receipt_id: Option<String>,
        outcome_ids: Vec<String>,
        source_event_ids: Vec<EventId>,
        status: GovernanceStatus,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: COMPANY_GOVERNANCE_SCHEMA.to_owned(),
            version: COMPANY_GOVERNANCE_SCHEMA_VERSION,
            project_id: project_id.into(),
            project_status,
            runtime_statuses,
            acceptance_status,
            review_id,
            delivery_status,
            closing_receipt_id,
            outcome_ids,
            source_event_ids,
            status,
            limitations,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_GOVERNANCE_SCHEMA
            || !self
                .version
                .is_compatible_with(&COMPANY_GOVERNANCE_SCHEMA_VERSION)
        {
            return Err("company_governance_schema_invalid".to_owned());
        }
        nonempty(&self.project_id, "company_governance_project_id", 256)?;
        if self.runtime_statuses.len() > MAX_GOVERNANCE_RUNS {
            return Err("company_governance_run_limit".to_owned());
        }
        for packet_id in self.runtime_statuses.keys() {
            nonempty(packet_id, "company_governance_packet_id", 256)?;
        }
        if self.source_event_ids.is_empty() || self.source_event_ids.len() > MAX_GOVERNANCE_REFS {
            return Err("company_governance_source_limit".to_owned());
        }
        let mut source_ids = BTreeSet::new();
        for event_id in &self.source_event_ids {
            if !source_ids.insert(event_id.to_string()) {
                return Err("company_governance_source_duplicate".to_owned());
            }
        }
        if self.outcome_ids.len() > MAX_GOVERNANCE_REFS {
            return Err("company_governance_outcome_limit".to_owned());
        }
        for outcome_id in &self.outcome_ids {
            nonempty(outcome_id, "company_governance_outcome_id", 256)?;
        }
        if let Some(review_id) = &self.review_id {
            nonempty(review_id, "company_governance_review_id", 256)?;
        }
        if let Some(receipt_id) = &self.closing_receipt_id {
            nonempty(receipt_id, "company_governance_closing_receipt_id", 256)?;
        }
        if self.status == GovernanceStatus::Closed {
            if self.project_status != ProjectStatus::Closed
                || !matches!(
                    self.acceptance_status,
                    Some(AcceptanceStatus::Accepted | AcceptanceStatus::Waived)
                )
                || self.delivery_status != Some(DeliveryStatus::Confirmed)
                || self.closing_receipt_id.is_none()
                || self.review_id.is_none()
                || self
                    .runtime_statuses
                    .values()
                    .any(|status| *status != ExecutionStatus::Completed)
            {
                return Err("company_governance_closed_chain_incomplete".to_owned());
            }
        }
        if self.status == GovernanceStatus::Accepted
            && !matches!(
                self.acceptance_status,
                Some(AcceptanceStatus::Accepted | AcceptanceStatus::Waived)
            )
        {
            return Err("company_governance_accepted_chain_incomplete".to_owned());
        }
        if self.status == GovernanceStatus::Unknown && self.limitations.is_empty() {
            return Err("company_governance_unknown_reason_required".to_owned());
        }
        if self.limitations.len() > MAX_GOVERNANCE_LIMITATIONS {
            return Err("company_governance_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            nonempty(limitation, "company_governance_limitation", 256)?;
        }
        digest(&self.snapshot_digest, "company_governance_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("company_governance_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "snapshot_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
