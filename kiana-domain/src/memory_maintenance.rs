//! Cache, index and retention maintenance plan contracts.
//!
//! A maintenance plan is a read-only, digest-bound decision. It may identify safe cleanup, an
//! orphan that needs recovery, or a quota shortfall, but it never removes an artifact, compacts an
//! index, advances a retention watermark or mutates EventLog. Adapters must recheck the same
//! Event/Receipt and retention references before any later effect.

use crate::{
    canonical_journal_bytes, json_digest, redact_text, RetentionDisposition, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MEMORY_MAINTENANCE_SCHEMA: &str = "kiana.memory-maintenance-plan.v1";
pub const MEMORY_MAINTENANCE_CANDIDATE_SCHEMA: &str = "kiana.memory-maintenance-candidate.v1";
pub const MEMORY_MAINTENANCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_CANDIDATES: usize = 1024;

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_contains_secret"));
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

fn clear_digest<T: Serialize>(value: &T, field: &str) -> String {
    let mut value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert(field.to_owned(), serde_json::Value::String(String::new()));
    }
    json_digest(&value)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceObjectKind {
    IndexGeneration,
    SummaryArtifact,
    ResultArtifact,
    CacheEntry,
    Tombstone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceDecision {
    Retain,
    DeleteEligible,
    ReportOrphan,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceCandidate {
    pub schema: String,
    pub object_id: String,
    pub kind: MaintenanceObjectKind,
    pub generation: u64,
    pub current_generation: u64,
    pub bytes: u64,
    pub observed_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub tombstone_cursor: Option<u64>,
    pub safe_tombstone_cursor: u64,
    pub retention: RetentionDisposition,
    pub legal_hold: bool,
    pub event_reference: Option<String>,
    pub receipt_reference: Option<String>,
    pub orphan_detected: bool,
    pub recoverable_orphan: bool,
    pub decision: MaintenanceDecision,
    pub candidate_digest: String,
}

impl MaintenanceCandidate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        object_id: impl Into<String>,
        kind: MaintenanceObjectKind,
        generation: u64,
        current_generation: u64,
        bytes: u64,
        observed_at_ms: u64,
        expires_at_ms: Option<u64>,
        tombstone_cursor: Option<u64>,
        safe_tombstone_cursor: u64,
        retention: RetentionDisposition,
        legal_hold: bool,
        event_reference: Option<String>,
        receipt_reference: Option<String>,
        orphan_detected: bool,
        recoverable_orphan: bool,
        decision: MaintenanceDecision,
    ) -> Result<Self, String> {
        let mut candidate = Self {
            schema: MEMORY_MAINTENANCE_CANDIDATE_SCHEMA.to_owned(),
            object_id: object_id.into(),
            kind,
            generation,
            current_generation,
            bytes,
            observed_at_ms,
            expires_at_ms,
            tombstone_cursor,
            safe_tombstone_cursor,
            retention,
            legal_hold,
            event_reference,
            receipt_reference,
            orphan_detected,
            recoverable_orphan,
            decision,
            candidate_digest: String::new(),
        };
        candidate.candidate_digest = candidate.digest();
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_MAINTENANCE_CANDIDATE_SCHEMA
            || self.bytes == 0
            || self.observed_at_ms == 0
            || self.current_generation == 0
            || self.safe_tombstone_cursor == 0
        {
            return Err("memory_maintenance_candidate_header_invalid".to_owned());
        }
        bounded(&self.object_id, "memory_maintenance_object_id", 512)?;
        if let Some(expires) = self.expires_at_ms {
            if expires == 0 {
                return Err("memory_maintenance_expiry_invalid".to_owned());
            }
        }
        for (reference, field) in [
            (&self.event_reference, "memory_maintenance_event_reference"),
            (
                &self.receipt_reference,
                "memory_maintenance_receipt_reference",
            ),
        ] {
            if let Some(reference) = reference {
                bounded(reference, field, 256)?;
            }
        }
        match self.kind {
            MaintenanceObjectKind::IndexGeneration => {
                if self.generation == 0
                    || self.expires_at_ms.is_some()
                    || self.tombstone_cursor.is_some()
                {
                    return Err("memory_maintenance_index_identity_invalid".to_owned());
                }
            }
            MaintenanceObjectKind::SummaryArtifact
            | MaintenanceObjectKind::ResultArtifact
            | MaintenanceObjectKind::CacheEntry => {
                if self.generation != 0 || self.tombstone_cursor.is_some() {
                    return Err("memory_maintenance_artifact_identity_invalid".to_owned());
                }
            }
            MaintenanceObjectKind::Tombstone => {
                if self.generation != 0
                    || self.expires_at_ms.is_some()
                    || self.tombstone_cursor.is_none()
                {
                    return Err("memory_maintenance_tombstone_identity_invalid".to_owned());
                }
            }
        }
        if self.orphan_detected
            && (self.event_reference.is_some() || self.receipt_reference.is_some())
        {
            return Err("memory_maintenance_orphan_reference_conflict".to_owned());
        }
        if self.decision == MaintenanceDecision::ReportOrphan
            && (!self.orphan_detected
                || !self.recoverable_orphan
                || self.event_reference.is_some()
                || self.receipt_reference.is_some())
        {
            return Err("memory_maintenance_orphan_report_invalid".to_owned());
        }
        if self.decision == MaintenanceDecision::DeleteEligible {
            if self.retention != RetentionDisposition::Eligible
                || self.legal_hold
                || self.orphan_detected
                || self.event_reference.is_some()
                || self.receipt_reference.is_some()
            {
                return Err("memory_maintenance_delete_reference_or_retention_gate".to_owned());
            }
            let eligible = match self.kind {
                MaintenanceObjectKind::IndexGeneration => self.generation < self.current_generation,
                MaintenanceObjectKind::SummaryArtifact
                | MaintenanceObjectKind::ResultArtifact
                | MaintenanceObjectKind::CacheEntry => self
                    .expires_at_ms
                    .is_some_and(|expires| expires <= self.observed_at_ms),
                MaintenanceObjectKind::Tombstone => self
                    .tombstone_cursor
                    .is_some_and(|cursor| cursor <= self.safe_tombstone_cursor),
            };
            if !eligible {
                return Err("memory_maintenance_delete_expiry_or_generation_gate".to_owned());
            }
        }
        if self.decision == MaintenanceDecision::Blocked
            && self.retention != RetentionDisposition::Unknown
            && !self.legal_hold
            && self.event_reference.is_none()
            && self.receipt_reference.is_none()
        {
            return Err("memory_maintenance_block_reason_missing".to_owned());
        }
        digest(
            &self.candidate_digest,
            "memory_maintenance_candidate_digest",
        )?;
        if self.candidate_digest != self.digest() {
            return Err("memory_maintenance_candidate_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "candidate_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMaintenancePlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub plan_id: String,
    pub retention_scan_digest: String,
    pub source_cursor: u64,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub observed_at_ms: u64,
    pub quota_bytes: u64,
    pub used_bytes: u64,
    pub candidates: Vec<MaintenanceCandidate>,
    pub planned_reclaim_bytes: u64,
    pub plan_digest: String,
}

impl MemoryMaintenancePlan {
    pub fn new(
        plan_id: impl Into<String>,
        retention_scan_digest: impl Into<String>,
        source_cursor: u64,
        policy_revision: u64,
        data_epoch: u64,
        observed_at_ms: u64,
        quota_bytes: u64,
        used_bytes: u64,
        candidates: Vec<MaintenanceCandidate>,
    ) -> Result<Self, String> {
        let planned_reclaim_bytes = candidates
            .iter()
            .filter(|candidate| candidate.decision == MaintenanceDecision::DeleteEligible)
            .try_fold(0_u64, |total, candidate| total.checked_add(candidate.bytes))
            .ok_or_else(|| "memory_maintenance_reclaim_overflow".to_owned())?;
        let mut plan = Self {
            schema: MEMORY_MAINTENANCE_SCHEMA.to_owned(),
            version: MEMORY_MAINTENANCE_VERSION,
            plan_id: plan_id.into(),
            retention_scan_digest: retention_scan_digest.into(),
            source_cursor,
            policy_revision,
            data_epoch,
            observed_at_ms,
            quota_bytes,
            used_bytes,
            candidates,
            planned_reclaim_bytes,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_MAINTENANCE_SCHEMA
            || self.version != MEMORY_MAINTENANCE_VERSION
            || self.source_cursor == 0
            || self.policy_revision == 0
            || self.data_epoch == 0
            || self.observed_at_ms == 0
            || self.quota_bytes == 0
            || self.candidates.is_empty()
            || self.candidates.len() > MAX_CANDIDATES
        {
            return Err("memory_maintenance_plan_header_invalid".to_owned());
        }
        bounded(&self.plan_id, "memory_maintenance_plan_id", 128)?;
        digest(
            &self.retention_scan_digest,
            "memory_maintenance_retention_scan_digest",
        )?;
        let mut ids = BTreeSet::new();
        let mut reclaim = 0_u64;
        for candidate in &self.candidates {
            candidate.validate()?;
            if !ids.insert(&candidate.object_id) {
                return Err("memory_maintenance_candidate_duplicate".to_owned());
            }
            if candidate.decision == MaintenanceDecision::DeleteEligible {
                reclaim = reclaim
                    .checked_add(candidate.bytes)
                    .ok_or_else(|| "memory_maintenance_reclaim_overflow".to_owned())?;
            }
        }
        if reclaim != self.planned_reclaim_bytes {
            return Err("memory_maintenance_reclaim_digest_mismatch".to_owned());
        }
        digest(&self.plan_digest, "memory_maintenance_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("memory_maintenance_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn quota_satisfied_after_plan(&self) -> Result<bool, String> {
        self.validate()?;
        Ok(self.used_bytes.saturating_sub(self.planned_reclaim_bytes) <= self.quota_bytes)
    }

    pub fn delete_candidates(&self) -> Result<Vec<String>, String> {
        self.validate()?;
        Ok(self
            .candidates
            .iter()
            .filter(|candidate| candidate.decision == MaintenanceDecision::DeleteEligible)
            .map(|candidate| candidate.object_id.clone())
            .collect())
    }

    pub fn orphan_candidates(&self) -> Result<Vec<String>, String> {
        self.validate()?;
        Ok(self
            .candidates
            .iter()
            .filter(|candidate| candidate.decision == MaintenanceDecision::ReportOrphan)
            .map(|candidate| candidate.object_id.clone())
            .collect())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "plan_digest")
    }
}
