//! Retention and legal-hold contracts shared by the control plane and EventLog adapters.
//!
//! A retention scan is a derived, rebuildable decision. It carries the policy revision and
//! source/projection cursors that produced it, while a legal-hold receipt proves that an active
//! hold was considered. The contract deliberately has no filesystem, clock or deletion side
//! effect; adapters must persist the receipt and later deletion work must consume this boundary.

use crate::{json_digest, DataClass, DataPayloadState, EventCursor, EventId, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const RETENTION_POLICY_SCHEMA: &str = "kiana.retention-policy.v1";
pub const LEGAL_HOLD_SCHEMA: &str = "kiana.legal-hold.v1";
pub const LEGAL_HOLD_RECEIPT_SCHEMA: &str = "kiana.legal-hold-receipt.v1";
pub const RETENTION_SCAN_SCHEMA: &str = "kiana.retention-scan.v1";
pub const RETENTION_CONTRACT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RETENTION_RULES: usize = 512;
pub const MAX_LEGAL_HOLD_OBJECTS: usize = 512;
pub const MAX_RETENTION_DECISIONS: usize = 1024;
pub const MAX_RETENTION_HOLDS: usize = 256;

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionDisposition {
    Retain,
    Eligible,
    Held,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicy {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_ref: String,
    pub revision: u64,
    pub data_epoch: u64,
    /// A finite default is mandatory. Absence is not interpreted as “retain forever”.
    pub default_retain_until_ms: u64,
    /// Object/source references may override the finite default window.
    pub rules: BTreeMap<String, u64>,
    pub policy_digest: String,
}

impl RetentionPolicy {
    pub fn new(
        project_ref: impl Into<String>,
        revision: u64,
        data_epoch: u64,
        default_retain_until_ms: u64,
        rules: BTreeMap<String, u64>,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: RETENTION_POLICY_SCHEMA.to_owned(),
            version: RETENTION_CONTRACT_VERSION,
            project_ref: project_ref.into(),
            revision,
            data_epoch,
            default_retain_until_ms,
            rules,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETENTION_POLICY_SCHEMA
            || !self.version.is_compatible_with(&RETENTION_CONTRACT_VERSION)
        {
            return Err("retention_policy_schema_invalid".to_owned());
        }
        nonempty(&self.project_ref, "retention_policy_project", 512)?;
        if self.data_epoch == 0 || self.default_retain_until_ms == 0 {
            return Err("retention_policy_finite_window_required".to_owned());
        }
        if self.rules.len() > MAX_RETENTION_RULES {
            return Err("retention_policy_rule_limit".to_owned());
        }
        for (object_ref, retain_until_ms) in &self.rules {
            nonempty(object_ref, "retention_policy_object_ref", 512)?;
            if *retain_until_ms == 0 {
                return Err("retention_policy_rule_window_invalid".to_owned());
            }
        }
        if !valid_digest(&self.policy_digest) {
            return Err("retention_policy_digest_invalid".to_owned());
        }
        if self.policy_digest != self.digest() {
            return Err("retention_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn retain_until_ms(&self, object_ref: &str) -> u64 {
        self.rules
            .get(object_ref)
            .copied()
            .unwrap_or(self.default_retain_until_ms)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "policy_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegalHold {
    pub schema: String,
    pub version: SchemaVersion,
    pub hold_id: String,
    pub project_ref: String,
    pub object_refs: BTreeSet<String>,
    pub reason: String,
    pub actor_ref: String,
    pub issued_at_ms: u64,
    pub policy_revision: u64,
    pub active: bool,
    pub hold_digest: String,
}

impl LegalHold {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        hold_id: impl Into<String>,
        project_ref: impl Into<String>,
        object_refs: BTreeSet<String>,
        reason: impl Into<String>,
        actor_ref: impl Into<String>,
        issued_at_ms: u64,
        policy_revision: u64,
        active: bool,
    ) -> Result<Self, String> {
        let mut hold = Self {
            schema: LEGAL_HOLD_SCHEMA.to_owned(),
            version: RETENTION_CONTRACT_VERSION,
            hold_id: hold_id.into(),
            project_ref: project_ref.into(),
            object_refs,
            reason: reason.into(),
            actor_ref: actor_ref.into(),
            issued_at_ms,
            policy_revision,
            active,
            hold_digest: String::new(),
        };
        hold.hold_digest = hold.digest();
        hold.validate()?;
        Ok(hold)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEGAL_HOLD_SCHEMA
            || !self.version.is_compatible_with(&RETENTION_CONTRACT_VERSION)
        {
            return Err("legal_hold_schema_invalid".to_owned());
        }
        nonempty(&self.hold_id, "legal_hold_id", 256)?;
        nonempty(&self.project_ref, "legal_hold_project", 512)?;
        nonempty(&self.reason, "legal_hold_reason", 1024)?;
        nonempty(&self.actor_ref, "legal_hold_actor", 256)?;
        if self.object_refs.is_empty() || self.object_refs.len() > MAX_LEGAL_HOLD_OBJECTS {
            return Err("legal_hold_object_limit".to_owned());
        }
        for object_ref in &self.object_refs {
            nonempty(object_ref, "legal_hold_object_ref", 512)?;
        }
        if self.issued_at_ms == 0 {
            return Err("legal_hold_issued_at_invalid".to_owned());
        }
        if !valid_digest(&self.hold_digest) {
            return Err("legal_hold_digest_invalid".to_owned());
        }
        if self.hold_digest != self.digest() {
            return Err("legal_hold_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn covers(&self, project_ref: &str, object_ref: &str) -> bool {
        self.active && self.project_ref == project_ref && self.object_refs.contains(object_ref)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "hold_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegalHoldReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub hold_id: String,
    pub project_ref: String,
    pub policy_revision: u64,
    pub source_cursor: EventCursor,
    pub projection_cursor: EventCursor,
    pub source_event_ids: Vec<EventId>,
    pub applied: bool,
    pub receipt_digest: String,
}

impl LegalHoldReceipt {
    pub fn new(
        hold: &LegalHold,
        source_cursor: EventCursor,
        projection_cursor: EventCursor,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        hold.validate()?;
        let mut receipt = Self {
            schema: LEGAL_HOLD_RECEIPT_SCHEMA.to_owned(),
            version: RETENTION_CONTRACT_VERSION,
            hold_id: hold.hold_id.clone(),
            project_ref: hold.project_ref.clone(),
            policy_revision: hold.policy_revision,
            source_cursor,
            projection_cursor,
            source_event_ids,
            applied: hold.active,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEGAL_HOLD_RECEIPT_SCHEMA
            || !self.version.is_compatible_with(&RETENTION_CONTRACT_VERSION)
        {
            return Err("legal_hold_receipt_schema_invalid".to_owned());
        }
        nonempty(&self.hold_id, "legal_hold_receipt_hold", 256)?;
        nonempty(&self.project_ref, "legal_hold_receipt_project", 512)?;
        if self.source_cursor == 0 || self.projection_cursor > self.source_cursor {
            return Err("legal_hold_receipt_cursor_invalid".to_owned());
        }
        if self.source_event_ids.is_empty()
            || self.source_event_ids.len() > crate::MAX_SOURCE_EVENT_IDS
        {
            return Err("legal_hold_receipt_source_ids_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for event_id in &self.source_event_ids {
            if !ids.insert(*event_id) {
                return Err("legal_hold_receipt_source_duplicate".to_owned());
            }
        }
        if !valid_digest(&self.receipt_digest) {
            return Err("legal_hold_receipt_digest_invalid".to_owned());
        }
        if self.receipt_digest != self.digest() {
            return Err("legal_hold_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "receipt_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionDecision {
    pub object_ref: String,
    pub class: DataClass,
    pub purpose_id: String,
    pub source_digest: String,
    pub payload: DataPayloadState,
    pub disposition: RetentionDisposition,
    pub retain_until_ms: u64,
    pub hold_id: Option<String>,
}

impl RetentionDecision {
    pub fn validate(&self) -> Result<(), String> {
        nonempty(&self.object_ref, "retention_decision_object_ref", 512)?;
        nonempty(&self.purpose_id, "retention_decision_purpose", 128)?;
        if !valid_digest(&self.source_digest) || self.retain_until_ms == 0 {
            return Err("retention_decision_evidence_invalid".to_owned());
        }
        if self.disposition == RetentionDisposition::Held && self.hold_id.is_none() {
            return Err("retention_decision_hold_missing".to_owned());
        }
        if let Some(hold_id) = &self.hold_id {
            nonempty(hold_id, "retention_decision_hold", 256)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionScan {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_ref: String,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub source_cursor: EventCursor,
    pub projection_cursor: EventCursor,
    pub source_event_ids: Vec<EventId>,
    pub decisions: Vec<RetentionDecision>,
    pub hold_receipts: Vec<LegalHoldReceipt>,
    pub scan_digest: String,
}

impl RetentionScan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_ref: impl Into<String>,
        policy_revision: u64,
        data_epoch: u64,
        source_cursor: EventCursor,
        projection_cursor: EventCursor,
        source_event_ids: Vec<EventId>,
        decisions: Vec<RetentionDecision>,
        hold_receipts: Vec<LegalHoldReceipt>,
    ) -> Result<Self, String> {
        let mut scan = Self {
            schema: RETENTION_SCAN_SCHEMA.to_owned(),
            version: RETENTION_CONTRACT_VERSION,
            project_ref: project_ref.into(),
            policy_revision,
            data_epoch,
            source_cursor,
            projection_cursor,
            source_event_ids,
            decisions,
            hold_receipts,
            scan_digest: String::new(),
        };
        scan.scan_digest = scan.digest();
        scan.validate()?;
        Ok(scan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETENTION_SCAN_SCHEMA
            || !self.version.is_compatible_with(&RETENTION_CONTRACT_VERSION)
        {
            return Err("retention_scan_schema_invalid".to_owned());
        }
        nonempty(&self.project_ref, "retention_scan_project", 512)?;
        if self.data_epoch == 0
            || self.source_cursor == 0
            || self.projection_cursor > self.source_cursor
        {
            return Err("retention_scan_cursor_invalid".to_owned());
        }
        if self.source_event_ids.is_empty()
            || self.source_event_ids.len() > crate::MAX_SOURCE_EVENT_IDS
        {
            return Err("retention_scan_source_ids_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for event_id in &self.source_event_ids {
            if !ids.insert(*event_id) {
                return Err("retention_scan_source_duplicate".to_owned());
            }
        }
        if self.decisions.len() > MAX_RETENTION_DECISIONS {
            return Err("retention_scan_decision_limit".to_owned());
        }
        let mut objects = BTreeSet::new();
        for decision in &self.decisions {
            decision.validate()?;
            if !objects.insert(decision.object_ref.clone()) {
                return Err("retention_scan_object_duplicate".to_owned());
            }
        }
        if self.hold_receipts.len() > MAX_RETENTION_HOLDS {
            return Err("retention_scan_hold_limit".to_owned());
        }
        let mut holds = BTreeSet::new();
        for receipt in &self.hold_receipts {
            receipt.validate()?;
            if receipt.project_ref != self.project_ref
                || receipt.policy_revision != self.policy_revision
                || receipt.source_cursor != self.source_cursor
                || receipt.projection_cursor != self.projection_cursor
                || receipt.source_event_ids != self.source_event_ids
                || !holds.insert(receipt.hold_id.clone())
            {
                return Err("retention_scan_hold_receipt_boundary_invalid".to_owned());
            }
        }
        for decision in &self.decisions {
            if decision.disposition == RetentionDisposition::Held
                && decision
                    .hold_id
                    .as_ref()
                    .is_none_or(|hold| !holds.contains(hold))
            {
                return Err("retention_scan_hold_receipt_missing".to_owned());
            }
        }
        if !valid_digest(&self.scan_digest) {
            return Err("retention_scan_digest_invalid".to_owned());
        }
        if self.scan_digest != self.digest() {
            return Err("retention_scan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "scan_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
