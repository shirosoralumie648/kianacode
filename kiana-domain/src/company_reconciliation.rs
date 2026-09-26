//! Company risk, incident and Unknown-effect reconciliation contracts.
//!
//! A risk trigger is only a signal.  A Company incident adds an owner, deadline and the exact
//! project object whose Unknown effect needs inspection.  Reconciliation appends an independent
//! observation; it never rewrites the original Run/Delivery result and never permits a blind retry.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_RECONCILIATION_SCHEMA: &str = "kiana.company-reconciliation.v1";

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

fn references(values: &[String], field: &'static str) -> Result<(), &'static str> {
    if values.is_empty() || values.len() > 256 {
        return Err(field);
    }
    if values.iter().any(|value| value.trim().is_empty())
        || values.iter().collect::<BTreeSet<_>>().len() != values.len()
    {
        return Err(field);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyRiskTriggerKind {
    MissingResult,
    WorkerLost,
    EvidenceCorrupt,
    BudgetUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyRiskTrigger {
    pub schema: String,
    pub trigger_id: String,
    pub project_id: String,
    #[serde(default)]
    pub packet_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub delivery_id: Option<String>,
    pub kind: CompanyRiskTriggerKind,
    pub source_ref: String,
    pub summary: String,
    pub observed_at: u64,
    pub digest: String,
}

impl CompanyRiskTrigger {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_RECONCILIATION_SCHEMA || self.observed_at == 0 {
            return Err("company_risk_trigger_header_invalid");
        }
        for (value, field) in [
            (&self.trigger_id, "company_risk_trigger_id_required"),
            (&self.project_id, "company_risk_trigger_project_required"),
            (&self.source_ref, "company_risk_trigger_source_required"),
            (&self.summary, "company_risk_trigger_summary_required"),
        ] {
            required(value, field)?;
        }
        if self.packet_id.is_none() && self.run_id.is_none() && self.delivery_id.is_none() {
            return Err("company_risk_trigger_target_required");
        }
        if self.run_id.is_some() && self.delivery_id.is_some() {
            return Err("company_risk_trigger_target_ambiguous");
        }
        for value in [
            self.packet_id.as_deref(),
            self.run_id.as_deref(),
            self.delivery_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            required(value, "company_risk_trigger_target_invalid")?;
        }
        digest(&self.digest, "company_risk_trigger_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_risk_trigger_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "trigger_id": self.trigger_id,
            "project_id": self.project_id,
            "packet_id": self.packet_id,
            "run_id": self.run_id,
            "delivery_id": self.delivery_id,
            "kind": self.kind,
            "source_ref": self.source_ref,
            "summary": self.summary,
            "observed_at": self.observed_at,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyIncidentState {
    Open,
    ReconciliationPending,
    Resolved,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyIncident {
    pub schema: String,
    pub incident_id: String,
    pub trigger_id: String,
    pub project_id: String,
    #[serde(default)]
    pub packet_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub delivery_id: Option<String>,
    pub owner_id: String,
    pub deadline_at: u64,
    pub original_unknown_digest: String,
    pub recovery_plan_ref: String,
    pub state: CompanyIncidentState,
    pub automatic_retry_allowed: bool,
    pub digest: String,
}

impl CompanyIncident {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_RECONCILIATION_SCHEMA
            || self.deadline_at == 0
            || self.automatic_retry_allowed
        {
            return Err("company_incident_header_invalid");
        }
        for (value, field) in [
            (&self.incident_id, "company_incident_id_required"),
            (&self.trigger_id, "company_incident_trigger_required"),
            (&self.project_id, "company_incident_project_required"),
            (&self.owner_id, "company_incident_owner_required"),
            (
                &self.recovery_plan_ref,
                "company_incident_recovery_plan_required",
            ),
        ] {
            required(value, field)?;
        }
        digest(
            &self.original_unknown_digest,
            "company_incident_unknown_digest_invalid",
        )?;
        if self.packet_id.is_none() && self.run_id.is_none() && self.delivery_id.is_none() {
            return Err("company_incident_target_required");
        }
        if self.run_id.is_some() && self.delivery_id.is_some() {
            return Err("company_incident_target_ambiguous");
        }
        for value in [
            self.packet_id.as_deref(),
            self.run_id.as_deref(),
            self.delivery_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            required(value, "company_incident_target_invalid")?;
        }
        digest(&self.digest, "company_incident_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_incident_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "incident_id": self.incident_id,
            "trigger_id": self.trigger_id,
            "project_id": self.project_id,
            "packet_id": self.packet_id,
            "run_id": self.run_id,
            "delivery_id": self.delivery_id,
            "owner_id": self.owner_id,
            "deadline_at": self.deadline_at,
            "original_unknown_digest": self.original_unknown_digest,
            "recovery_plan_ref": self.recovery_plan_ref,
            "state": self.state,
            "automatic_retry_allowed": self.automatic_retry_allowed,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyObservationKind {
    Run,
    Delivery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyObservationSource {
    IndependentQuery,
    ManualEvidence,
    StopReceipt,
    ModelReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyObservationOutcome {
    Unknown,
    ConfirmedSucceeded,
    ConfirmedFailed,
    NoEffect,
}

/// A read-only observation candidate. `Unknown`, model claims and status reports may be recorded
/// as rejected input, but `validate_for_reconciliation` never accepts them as resolution evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyEffectObservation {
    pub schema: String,
    pub observation_id: String,
    pub kind: CompanyObservationKind,
    pub project_id: String,
    #[serde(default)]
    pub packet_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub delivery_id: Option<String>,
    pub target_ref: String,
    pub account_ref: String,
    pub scope_digest: String,
    pub baseline_version: u64,
    pub authority_epoch: u64,
    pub original_unknown_digest: String,
    pub source: CompanyObservationSource,
    pub outcome: CompanyObservationOutcome,
    pub evidence_refs: Vec<String>,
    pub stop_confirmed: bool,
    pub model_claimed: bool,
    pub status_report: bool,
    pub observed_at: u64,
    pub digest: String,
}

impl CompanyEffectObservation {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_RECONCILIATION_SCHEMA
            || self.baseline_version == 0
            || self.authority_epoch == 0
            || self.observed_at == 0
        {
            return Err("company_observation_header_invalid");
        }
        for (value, field) in [
            (&self.observation_id, "company_observation_id_required"),
            (&self.project_id, "company_observation_project_required"),
            (&self.target_ref, "company_observation_target_required"),
            (&self.account_ref, "company_observation_account_required"),
        ] {
            required(value, field)?;
        }
        digest(
            &self.scope_digest,
            "company_observation_scope_digest_invalid",
        )?;
        digest(
            &self.original_unknown_digest,
            "company_observation_unknown_digest_invalid",
        )?;
        references(&self.evidence_refs, "company_observation_evidence_required")?;
        match self.kind {
            CompanyObservationKind::Run if self.run_id.is_none() || self.delivery_id.is_some() => {
                return Err("company_observation_run_identity_invalid");
            }
            CompanyObservationKind::Delivery
                if self.delivery_id.is_none() || self.run_id.is_some() =>
            {
                return Err("company_observation_delivery_identity_invalid");
            }
        }
        if self.packet_id.is_none() {
            return Err("company_observation_packet_required");
        }
        for value in [
            self.packet_id.as_deref(),
            self.run_id.as_deref(),
            self.delivery_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            required(value, "company_observation_identity_invalid")?;
        }
        digest(&self.digest, "company_observation_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_observation_digest_mismatch");
        }
        Ok(())
    }

    fn validate_for_reconciliation(
        &self,
        case: &CompanyReconciliationCase,
    ) -> Result<(), &'static str> {
        self.validate()?;
        if self.project_id != case.project_id
            || self.packet_id != case.packet_id
            || self.run_id != case.run_id
            || self.delivery_id != case.delivery_id
            || self.target_ref != case.target_ref
            || self.account_ref != case.account_ref
            || self.scope_digest != case.scope_digest
            || self.baseline_version != case.baseline_version
            || self.authority_epoch != case.authority_epoch
            || self.original_unknown_digest != case.original_unknown_digest
        {
            return Err("company_reconciliation_evidence_binding_mismatch");
        }
        if self.source == CompanyObservationSource::ModelReport
            || self.model_claimed
            || self.status_report
        {
            return Err("company_reconciliation_model_or_status_claim_forbidden");
        }
        if self.source == CompanyObservationSource::StopReceipt && !self.stop_confirmed {
            return Err("company_reconciliation_stop_unconfirmed");
        }
        if !self.stop_confirmed {
            return Err("company_reconciliation_stop_unconfirmed");
        }
        if self.outcome == CompanyObservationOutcome::Unknown {
            return Err("company_reconciliation_unknown_evidence_forbidden");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "observation_id": self.observation_id,
            "kind": self.kind,
            "project_id": self.project_id,
            "packet_id": self.packet_id,
            "run_id": self.run_id,
            "delivery_id": self.delivery_id,
            "target_ref": self.target_ref,
            "account_ref": self.account_ref,
            "scope_digest": self.scope_digest,
            "baseline_version": self.baseline_version,
            "authority_epoch": self.authority_epoch,
            "original_unknown_digest": self.original_unknown_digest,
            "source": self.source,
            "outcome": self.outcome,
            "evidence_refs": self.evidence_refs,
            "stop_confirmed": self.stop_confirmed,
            "model_claimed": self.model_claimed,
            "status_report": self.status_report,
            "observed_at": self.observed_at,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReconciliationEvidence {
    pub schema: String,
    pub observation_id: String,
    pub source: CompanyObservationSource,
    pub outcome: CompanyObservationOutcome,
    pub evidence_refs: Vec<String>,
    pub observation_digest: String,
    pub digest: String,
}

impl CompanyReconciliationEvidence {
    fn from_observation(observation: &CompanyEffectObservation) -> Self {
        let mut evidence = Self {
            schema: COMPANY_RECONCILIATION_SCHEMA.to_owned(),
            observation_id: observation.observation_id.clone(),
            source: observation.source,
            outcome: observation.outcome,
            evidence_refs: observation.evidence_refs.clone(),
            observation_digest: observation.digest.clone(),
            digest: String::new(),
        };
        evidence.digest = evidence.canonical_digest();
        evidence
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_RECONCILIATION_SCHEMA
            || self.outcome == CompanyObservationOutcome::Unknown
            || self.source == CompanyObservationSource::ModelReport
        {
            return Err("company_reconciliation_evidence_invalid");
        }
        required(
            &self.observation_id,
            "company_reconciliation_evidence_observation_required",
        )?;
        digest(
            &self.observation_digest,
            "company_reconciliation_evidence_observation_digest_invalid",
        )?;
        references(
            &self.evidence_refs,
            "company_reconciliation_evidence_refs_required",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("company_reconciliation_evidence_digest_mismatch");
        }
        Ok(())
    }

    fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "observation_id": self.observation_id,
            "source": self.source,
            "outcome": self.outcome,
            "evidence_refs": self.evidence_refs,
            "observation_digest": self.observation_digest,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyReconciliationState {
    Pending,
    EvidenceAttached,
    Reconciled,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReconciliationCase {
    pub schema: String,
    pub case_id: String,
    pub incident_id: String,
    pub project_id: String,
    pub packet_id: Option<String>,
    pub run_id: Option<String>,
    pub delivery_id: Option<String>,
    pub target_ref: String,
    pub account_ref: String,
    pub scope_digest: String,
    pub baseline_version: u64,
    pub authority_epoch: u64,
    pub original_unknown_digest: String,
    pub state: CompanyReconciliationState,
    pub reconciliation_required: bool,
    pub automatic_retry_allowed: bool,
    pub evidence: Option<CompanyReconciliationEvidence>,
    pub dependent_work_blocked: bool,
    #[serde(default)]
    pub failure_reason: Option<String>,
    pub case_digest: String,
}

impl CompanyReconciliationCase {
    pub fn from_incident(
        case_id: impl Into<String>,
        incident: &CompanyIncident,
        target_ref: impl Into<String>,
        account_ref: impl Into<String>,
        scope_digest: impl Into<String>,
        baseline_version: u64,
        authority_epoch: u64,
    ) -> Result<Self, &'static str> {
        incident.validate()?;
        let mut case = Self {
            schema: COMPANY_RECONCILIATION_SCHEMA.to_owned(),
            case_id: case_id.into(),
            incident_id: incident.incident_id.clone(),
            project_id: incident.project_id.clone(),
            packet_id: incident.packet_id.clone(),
            run_id: incident.run_id.clone(),
            delivery_id: incident.delivery_id.clone(),
            target_ref: target_ref.into(),
            account_ref: account_ref.into(),
            scope_digest: scope_digest.into(),
            baseline_version,
            authority_epoch,
            original_unknown_digest: incident.original_unknown_digest.clone(),
            state: CompanyReconciliationState::Pending,
            reconciliation_required: true,
            automatic_retry_allowed: false,
            evidence: None,
            dependent_work_blocked: true,
            failure_reason: None,
            case_digest: String::new(),
        };
        case.case_digest = case.canonical_digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_RECONCILIATION_SCHEMA
            || self.baseline_version == 0
            || self.authority_epoch == 0
            || !self.reconciliation_required
            || self.automatic_retry_allowed
        {
            return Err("company_reconciliation_case_header_invalid");
        }
        for (value, field) in [
            (&self.case_id, "company_reconciliation_case_id_required"),
            (
                &self.incident_id,
                "company_reconciliation_incident_required",
            ),
            (&self.project_id, "company_reconciliation_project_required"),
            (&self.target_ref, "company_reconciliation_target_required"),
            (&self.account_ref, "company_reconciliation_account_required"),
        ] {
            required(value, field)?;
        }
        digest(&self.scope_digest, "company_reconciliation_scope_invalid")?;
        digest(
            &self.original_unknown_digest,
            "company_reconciliation_unknown_digest_invalid",
        )?;
        if self.packet_id.is_none() && self.run_id.is_none() && self.delivery_id.is_none() {
            return Err("company_reconciliation_target_identity_required");
        }
        if self.run_id.is_some() && self.delivery_id.is_some() {
            return Err("company_reconciliation_target_ambiguous");
        }
        if self.evidence.is_none() && self.state != CompanyReconciliationState::Pending {
            return Err("company_reconciliation_evidence_missing");
        }
        if let Some(evidence) = &self.evidence {
            evidence.validate()?;
        }
        if self.state == CompanyReconciliationState::Pending && self.evidence.is_some() {
            return Err("company_reconciliation_pending_evidence_invalid");
        }
        if matches!(
            self.state,
            CompanyReconciliationState::Pending | CompanyReconciliationState::EvidenceAttached
        ) && !self.dependent_work_blocked
        {
            return Err("company_reconciliation_unresolved_dependents_must_block");
        }
        if self.state == CompanyReconciliationState::Reconciled && self.dependent_work_blocked {
            return Err("company_reconciliation_resolved_dependents_blocked");
        }
        if self.state == CompanyReconciliationState::Failed && !self.dependent_work_blocked {
            return Err("company_reconciliation_failure_must_block_dependents");
        }
        if let Some(reason) = &self.failure_reason {
            required(reason, "company_reconciliation_failure_reason_invalid")?;
            if self.state != CompanyReconciliationState::Failed {
                return Err("company_reconciliation_failure_reason_state_invalid");
            }
        }
        if self.case_digest != self.canonical_digest() {
            return Err("company_reconciliation_case_digest_mismatch");
        }
        Ok(())
    }

    pub fn attach_observation(
        &self,
        observation: CompanyEffectObservation,
    ) -> Result<Self, &'static str> {
        self.validate()?;
        if self.state != CompanyReconciliationState::Pending {
            return Err("company_reconciliation_case_not_pending");
        }
        observation.validate_for_reconciliation(self)?;
        let mut next = self.clone();
        next.state = CompanyReconciliationState::EvidenceAttached;
        next.evidence = Some(CompanyReconciliationEvidence::from_observation(
            &observation,
        ));
        next.case_digest = next.canonical_digest();
        next.validate()?;
        Ok(next)
    }

    pub fn commit_reconciled(&self) -> Result<Self, &'static str> {
        self.validate()?;
        if self.state != CompanyReconciliationState::EvidenceAttached {
            return Err("company_reconciliation_evidence_required");
        }
        if self
            .evidence
            .as_ref()
            .is_none_or(|evidence| evidence.outcome == CompanyObservationOutcome::ConfirmedFailed)
        {
            return Err("company_reconciliation_failure_requires_block");
        }
        let mut next = self.clone();
        next.state = CompanyReconciliationState::Reconciled;
        next.dependent_work_blocked = false;
        next.case_digest = next.canonical_digest();
        next.validate()?;
        Ok(next)
    }

    pub fn commit_failed(&self, reason: impl Into<String>) -> Result<Self, &'static str> {
        self.validate()?;
        if self.state != CompanyReconciliationState::EvidenceAttached
            || self.evidence.as_ref().is_none_or(|evidence| {
                evidence.outcome != CompanyObservationOutcome::ConfirmedFailed
            })
        {
            return Err("company_reconciliation_failed_outcome_required");
        }
        let reason = reason.into();
        required(&reason, "company_reconciliation_failure_reason_invalid")?;
        let mut next = self.clone();
        next.state = CompanyReconciliationState::Failed;
        next.dependent_work_blocked = true;
        next.failure_reason = Some(reason);
        next.case_digest = next.canonical_digest();
        next.validate()?;
        Ok(next)
    }

    pub fn can_resume_dependents(&self) -> bool {
        self.state == CompanyReconciliationState::Reconciled && !self.dependent_work_blocked
    }

    fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "case_id": self.case_id,
            "incident_id": self.incident_id,
            "project_id": self.project_id,
            "packet_id": self.packet_id,
            "run_id": self.run_id,
            "delivery_id": self.delivery_id,
            "target_ref": self.target_ref,
            "account_ref": self.account_ref,
            "scope_digest": self.scope_digest,
            "baseline_version": self.baseline_version,
            "authority_epoch": self.authority_epoch,
            "original_unknown_digest": self.original_unknown_digest,
            "state": self.state,
            "reconciliation_required": self.reconciliation_required,
            "automatic_retry_allowed": self.automatic_retry_allowed,
            "evidence": self.evidence,
            "dependent_work_blocked": self.dependent_work_blocked,
            "failure_reason": self.failure_reason,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompanyReconciliationLedger {
    pub triggers: BTreeMap<String, CompanyRiskTrigger>,
    pub incidents: BTreeMap<String, CompanyIncident>,
    pub cases: BTreeMap<String, Vec<CompanyReconciliationCase>>,
}

impl CompanyReconciliationLedger {
    pub fn record_trigger(&mut self, trigger: CompanyRiskTrigger) -> Result<(), &'static str> {
        trigger.validate()?;
        if let Some(existing) = self.triggers.get(&trigger.trigger_id) {
            if existing.digest == trigger.digest {
                return Ok(());
            }
            return Err("company_risk_trigger_duplicate_digest_mismatch");
        }
        self.triggers.insert(trigger.trigger_id.clone(), trigger);
        Ok(())
    }

    pub fn open_incident(&mut self, incident: CompanyIncident) -> Result<(), &'static str> {
        incident.validate()?;
        let trigger = self
            .triggers
            .get(&incident.trigger_id)
            .ok_or("company_incident_trigger_not_found")?;
        if trigger.project_id != incident.project_id
            || trigger.packet_id != incident.packet_id
            || trigger.run_id != incident.run_id
            || trigger.delivery_id != incident.delivery_id
        {
            return Err("company_incident_trigger_binding_mismatch");
        }
        if let Some(existing) = self.incidents.get(&incident.incident_id) {
            if existing.digest == incident.digest {
                return Ok(());
            }
            return Err("company_incident_duplicate_digest_mismatch");
        }
        self.incidents
            .insert(incident.incident_id.clone(), incident);
        Ok(())
    }

    pub fn record_case(&mut self, case: CompanyReconciliationCase) -> Result<(), &'static str> {
        case.validate()?;
        let incident = self
            .incidents
            .get(&case.incident_id)
            .ok_or("company_reconciliation_incident_not_found")?;
        if incident.project_id != case.project_id
            || incident.packet_id != case.packet_id
            || incident.run_id != case.run_id
            || incident.delivery_id != case.delivery_id
            || incident.original_unknown_digest != case.original_unknown_digest
        {
            return Err("company_reconciliation_incident_binding_mismatch");
        }
        let history = self.cases.entry(case.case_id.clone()).or_default();
        if let Some(previous) = history.last() {
            if previous.case_digest == case.case_digest {
                return Ok(());
            }
            let valid_transition = matches!(
                (previous.state, case.state),
                (
                    CompanyReconciliationState::Pending,
                    CompanyReconciliationState::EvidenceAttached
                ) | (
                    CompanyReconciliationState::EvidenceAttached,
                    CompanyReconciliationState::Reconciled | CompanyReconciliationState::Failed
                )
            );
            if !valid_transition || previous.original_unknown_digest != case.original_unknown_digest
            {
                return Err("company_reconciliation_case_transition_invalid");
            }
        }
        history.push(case);
        Ok(())
    }

    pub fn latest_case(&self, case_id: &str) -> Option<&CompanyReconciliationCase> {
        self.cases.get(case_id).and_then(|history| history.last())
    }
}
