//! Durable-fact Company restart hydration and explicit recovery planning.
//!
//! Hydration is read-only and fail-closed: cursor gaps, unknown schemas, corrupt evidence and
//! Unknown effects leave the project paused. Resume is a new, approved plan, never blind replay.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_RECOVERY_SCHEMA: &str = "kiana.company-recovery.v1";
pub const COMPANY_FACT_SCHEMA: &str = "kiana.company-fact.v1";
pub const COMPANY_RECOVERY_SCHEMA_VERSION: u16 = 1;

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyFactKind {
    Intent,
    Dispatch,
    RuntimeObservation,
    HumanDecision,
    DeliveryObservation,
    Closing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyFactOutcome {
    Known,
    ResultUnknown,
    EvidenceCorrupt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyRecoveryFact {
    pub schema: String,
    pub schema_version: u16,
    pub project_id: String,
    pub sequence: u64,
    pub source_cursor: u64,
    pub kind: CompanyFactKind,
    pub outcome: CompanyFactOutcome,
    pub payload_digest: String,
    pub evidence_digest: Option<String>,
    pub digest: String,
}

impl CompanyRecoveryFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_FACT_SCHEMA
            || self.schema_version != COMPANY_RECOVERY_SCHEMA_VERSION
            || self.sequence == 0
            || self.source_cursor == 0
        {
            return Err("company_recovery_fact_header_invalid");
        }
        required(&self.project_id, "company_recovery_fact_project_required")?;
        digest(
            &self.payload_digest,
            "company_recovery_fact_payload_invalid",
        )?;
        if let Some(evidence) = &self.evidence_digest {
            digest(evidence, "company_recovery_fact_evidence_invalid")?;
        }
        if self.outcome == CompanyFactOutcome::Known && self.evidence_digest.is_none() {
            return Err("company_recovery_fact_evidence_required");
        }
        digest(&self.digest, "company_recovery_fact_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_recovery_fact_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "schema_version": self.schema_version,
            "project_id": self.project_id,
            "sequence": self.sequence,
            "source_cursor": self.source_cursor,
            "kind": self.kind,
            "outcome": self.outcome,
            "payload_digest": self.payload_digest,
            "evidence_digest": self.evidence_digest,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyRecoveryState {
    Paused,
    NeedsReconciliation,
    ReadyForApproval,
    ResumedPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyRecoverySnapshot {
    pub schema: String,
    pub project_id: String,
    pub source_cursor: u64,
    pub data_epoch: u64,
    pub authority_epoch: u64,
    pub project_revision: u64,
    pub state: CompanyRecoveryState,
    pub default_paused: bool,
    pub unknown_fact_sequences: Vec<u64>,
    pub pending_dispatch_sequences: Vec<u64>,
    pub pending_decision_sequences: Vec<u64>,
    pub fact_digests: Vec<String>,
    pub snapshot_digest: String,
}

impl CompanyRecoverySnapshot {
    pub fn hydrate(
        project_id: impl Into<String>,
        data_epoch: u64,
        authority_epoch: u64,
        project_revision: u64,
        facts: &[CompanyRecoveryFact],
    ) -> Result<Self, &'static str> {
        let project_id = project_id.into();
        if data_epoch == 0 || authority_epoch == 0 || project_revision == 0 || facts.is_empty() {
            return Err("company_recovery_hydration_header_invalid");
        }
        let mut expected_sequence = 1;
        let mut source_cursor = 0;
        let mut unknown = Vec::new();
        let mut pending_dispatch = Vec::new();
        let mut pending_decisions = Vec::new();
        let mut digests = Vec::new();
        for fact in facts {
            fact.validate()?;
            if fact.project_id != project_id || fact.sequence != expected_sequence {
                return Err("company_recovery_fact_gap_or_project_mismatch");
            }
            if fact.source_cursor <= source_cursor {
                return Err("company_recovery_cursor_regressed");
            }
            source_cursor = fact.source_cursor;
            expected_sequence = expected_sequence.saturating_add(1);
            digests.push(fact.digest.clone());
            match fact.kind {
                CompanyFactKind::Dispatch if fact.outcome == CompanyFactOutcome::Known => {
                    pending_dispatch.push(fact.sequence)
                }
                CompanyFactKind::HumanDecision if fact.outcome == CompanyFactOutcome::Known => {
                    pending_decisions.push(fact.sequence)
                }
                _ => {}
            }
            if matches!(
                fact.outcome,
                CompanyFactOutcome::ResultUnknown | CompanyFactOutcome::EvidenceCorrupt
            ) {
                unknown.push(fact.sequence);
            }
        }
        let state = if unknown.is_empty() {
            CompanyRecoveryState::Paused
        } else {
            CompanyRecoveryState::NeedsReconciliation
        };
        let mut snapshot = Self {
            schema: COMPANY_RECOVERY_SCHEMA.to_owned(),
            project_id,
            source_cursor,
            data_epoch,
            authority_epoch,
            project_revision,
            state,
            default_paused: true,
            unknown_fact_sequences: unknown,
            pending_dispatch_sequences: pending_dispatch,
            pending_decision_sequences: pending_decisions,
            fact_digests: digests,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.canonical_digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_RECOVERY_SCHEMA
            || self.source_cursor == 0
            || self.data_epoch == 0
            || self.authority_epoch == 0
            || self.project_revision == 0
            || !self.default_paused
            || self.fact_digests.is_empty()
        {
            return Err("company_recovery_snapshot_invalid");
        }
        required(
            &self.project_id,
            "company_recovery_snapshot_project_required",
        )?;
        if self.fact_digests.iter().any(|fact_digest| {
            digest(fact_digest, "company_recovery_snapshot_fact_digest_invalid").is_err()
        }) {
            return Err("company_recovery_snapshot_fact_digest_invalid");
        }
        if self.unknown_fact_sequences.is_empty()
            && self.state == CompanyRecoveryState::NeedsReconciliation
        {
            return Err("company_recovery_snapshot_state_invalid");
        }
        digest(
            &self.snapshot_digest,
            "company_recovery_snapshot_digest_invalid",
        )?;
        if self.snapshot_digest != self.canonical_digest() {
            return Err("company_recovery_snapshot_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "project_id": self.project_id,
            "source_cursor": self.source_cursor,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "project_revision": self.project_revision,
            "state": self.state,
            "default_paused": self.default_paused,
            "unknown_fact_sequences": self.unknown_fact_sequences,
            "pending_dispatch_sequences": self.pending_dispatch_sequences,
            "pending_decision_sequences": self.pending_decision_sequences,
            "fact_digests": self.fact_digests,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyRecoveryAction {
    ReadOnly,
    Reconcile,
    Resume,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyRecoveryPlan {
    pub schema: String,
    pub plan_id: String,
    pub project_id: String,
    pub snapshot_digest: String,
    pub data_epoch: u64,
    pub authority_epoch: u64,
    pub action: CompanyRecoveryAction,
    pub approval_ref: Option<String>,
    pub automatic_retry_allowed: bool,
    pub digest: String,
}

impl CompanyRecoveryPlan {
    pub fn new(
        plan_id: impl Into<String>,
        snapshot: &CompanyRecoverySnapshot,
        action: CompanyRecoveryAction,
        approval_ref: Option<String>,
    ) -> Result<Self, &'static str> {
        snapshot.validate()?;
        let plan_id = plan_id.into();
        let mut plan = Self {
            schema: COMPANY_RECOVERY_SCHEMA.to_owned(),
            plan_id,
            project_id: snapshot.project_id.clone(),
            snapshot_digest: snapshot.snapshot_digest.clone(),
            data_epoch: snapshot.data_epoch,
            authority_epoch: snapshot.authority_epoch,
            action,
            approval_ref,
            automatic_retry_allowed: false,
            digest: String::new(),
        };
        plan.digest = plan.canonical_digest();
        plan.validate_against(snapshot)?;
        Ok(plan)
    }

    pub fn validate_against(&self, snapshot: &CompanyRecoverySnapshot) -> Result<(), &'static str> {
        snapshot.validate()?;
        if self.schema != COMPANY_RECOVERY_SCHEMA
            || self.project_id != snapshot.project_id
            || self.snapshot_digest != snapshot.snapshot_digest
            || self.data_epoch != snapshot.data_epoch
            || self.authority_epoch != snapshot.authority_epoch
            || self.automatic_retry_allowed
        {
            return Err("company_recovery_plan_binding_invalid");
        }
        required(&self.plan_id, "company_recovery_plan_id_required")?;
        match self.action {
            CompanyRecoveryAction::ReadOnly => {}
            CompanyRecoveryAction::Reconcile => {}
            CompanyRecoveryAction::Resume => {
                if snapshot.state != CompanyRecoveryState::Paused
                    || !snapshot.unknown_fact_sequences.is_empty()
                    || self.approval_ref.as_deref().is_none_or(str::is_empty)
                {
                    return Err("company_recovery_resume_blocked");
                }
            }
        }
        digest(
            &self.snapshot_digest,
            "company_recovery_plan_snapshot_digest_invalid",
        )?;
        digest(&self.digest, "company_recovery_plan_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_recovery_plan_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "plan_id": self.plan_id,
            "project_id": self.project_id,
            "snapshot_digest": self.snapshot_digest,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "action": self.action,
            "approval_ref": self.approval_ref,
            "automatic_retry_allowed": self.automatic_retry_allowed,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompanyRecoveryLedger {
    pub snapshots: BTreeMap<String, CompanyRecoverySnapshot>,
    pub plans: BTreeMap<String, CompanyRecoveryPlan>,
}

impl CompanyRecoveryLedger {
    pub fn record_snapshot(
        &mut self,
        snapshot: CompanyRecoverySnapshot,
    ) -> Result<(), &'static str> {
        snapshot.validate()?;
        if let Some(existing) = self.snapshots.get(&snapshot.project_id) {
            if existing.snapshot_digest == snapshot.snapshot_digest {
                return Ok(());
            }
            return Err("company_recovery_snapshot_duplicate_digest_mismatch");
        }
        self.snapshots.insert(snapshot.project_id.clone(), snapshot);
        Ok(())
    }

    pub fn record_plan(&mut self, plan: CompanyRecoveryPlan) -> Result<(), &'static str> {
        let snapshot = self
            .snapshots
            .get(&plan.project_id)
            .ok_or("company_recovery_snapshot_not_found")?;
        plan.validate_against(snapshot)?;
        if let Some(existing) = self.plans.get(&plan.plan_id) {
            if existing.digest == plan.digest {
                return Ok(());
            }
            return Err("company_recovery_plan_duplicate_digest_mismatch");
        }
        self.plans.insert(plan.plan_id.clone(), plan);
        Ok(())
    }
}
