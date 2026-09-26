//! Honest Company closing receipts for success, failure, cancellation and waiver.
//!
//! This contract aggregates packet/attempt, run, review/acceptance, delivery, incident and
//! evidence references. It is a source projection only: the existing Company transition and
//! EventLog remain the authority for committing effects.

use crate::{json_digest, AcceptanceStatus, DeliveryStatus, ProjectStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_CLOSING_RECEIPT_SCHEMA: &str = "kiana.company-closing-receipt.v2";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn references(
    values: &[String],
    field: &'static str,
    allow_empty: bool,
) -> Result<(), &'static str> {
    if (!allow_empty && values.is_empty()) || values.len() > 512 {
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
pub enum CompanyCloseKind {
    Success,
    Failure,
    Cancelled,
    Waived,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyClosingReceiptContract {
    pub schema: String,
    pub receipt_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub close_kind: CompanyCloseKind,
    pub project_status: ProjectStatus,
    pub acceptance_status: AcceptanceStatus,
    pub delivery_status: Option<DeliveryStatus>,
    pub reason: String,
    pub packet_attempt_refs: BTreeMap<String, String>,
    pub run_refs: Vec<String>,
    pub review_refs: Vec<String>,
    pub acceptance_refs: Vec<String>,
    pub delivery_refs: Vec<String>,
    pub incident_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub residual_obligations: Vec<String>,
    pub all_runs_stopped: bool,
    pub unresolved_incidents: bool,
    pub authors: Vec<String>,
    pub reviewer_refs: Vec<String>,
    pub closer_id: String,
    #[serde(default)]
    pub waiver_ref: Option<String>,
    #[serde(default)]
    pub waiver_by: Option<String>,
    pub closed_at: u64,
    pub digest: String,
}

impl CompanyClosingReceiptContract {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_CLOSING_RECEIPT_SCHEMA
            || self.baseline_version == 0
            || self.closed_at == 0
        {
            return Err("closing_receipt_header_invalid");
        }
        for (value, field) in [
            (&self.receipt_id, "closing_receipt_id_required"),
            (&self.project_id, "closing_receipt_project_required"),
            (&self.reason, "closing_receipt_reason_required"),
            (&self.closer_id, "closing_receipt_closer_required"),
        ] {
            required(value, field)?;
        }
        if self.packet_attempt_refs.is_empty() {
            return Err("closing_receipt_packet_attempts_required");
        }
        if self.packet_attempt_refs.len() > 512 {
            return Err("closing_receipt_packet_attempts_limit");
        }
        for (packet, attempt) in &self.packet_attempt_refs {
            required(packet, "closing_receipt_packet_ref_invalid")?;
            required(attempt, "closing_receipt_attempt_ref_invalid")?;
        }
        references(&self.run_refs, "closing_receipt_runs_required", false)?;
        references(&self.review_refs, "closing_receipt_reviews_required", false)?;
        references(
            &self.acceptance_refs,
            "closing_receipt_acceptances_required",
            false,
        )?;
        references(
            &self.delivery_refs,
            "closing_receipt_deliveries_required",
            self.close_kind != CompanyCloseKind::Success,
        )?;
        references(
            &self.incident_refs,
            "closing_receipt_incidents_invalid",
            true,
        )?;
        references(
            &self.evidence_refs,
            "closing_receipt_evidence_required",
            false,
        )?;
        references(
            &self.residual_obligations,
            "closing_receipt_residual_invalid",
            true,
        )?;
        if self.authors.is_empty() || self.authors.len() > 512 {
            return Err("closing_receipt_authors_required");
        }
        for author in &self.authors {
            required(author, "closing_receipt_author_invalid")?;
        }
        references(
            &self.reviewer_refs,
            "closing_receipt_reviewers_required",
            false,
        )?;
        if self.authors.iter().any(|author| author == &self.closer_id)
            || self
                .reviewer_refs
                .iter()
                .any(|reviewer| reviewer == &self.closer_id)
            || self.authors.iter().collect::<BTreeSet<_>>().len() != self.authors.len()
        {
            return Err("closing_receipt_role_independence_invalid");
        }
        if self.unresolved_incidents {
            return Err("closing_receipt_unresolved_incident");
        }
        match self.close_kind {
            CompanyCloseKind::Success => {
                if !matches!(
                    self.project_status,
                    ProjectStatus::Accepted | ProjectStatus::Closed
                ) || self.acceptance_status != AcceptanceStatus::Accepted
                    || self.delivery_status != Some(DeliveryStatus::Confirmed)
                    || !self.all_runs_stopped
                    || !self.residual_obligations.is_empty()
                    || self.waiver_ref.is_some()
                    || self.waiver_by.is_some()
                {
                    return Err("closing_success_chain_incomplete");
                }
            }
            CompanyCloseKind::Failure => {
                if !matches!(
                    self.project_status,
                    ProjectStatus::Active
                        | ProjectStatus::AtRisk
                        | ProjectStatus::ReadyForAcceptance
                        | ProjectStatus::Failed
                ) || !self.all_runs_stopped
                {
                    return Err("closing_failure_stop_or_status_invalid");
                }
                if self.waiver_ref.is_some() || self.waiver_by.is_some() {
                    return Err("closing_failure_waiver_forbidden");
                }
            }
            CompanyCloseKind::Cancelled => {
                if self.project_status != ProjectStatus::Cancelled || !self.all_runs_stopped {
                    return Err("closing_cancel_stop_required");
                }
                if self.waiver_ref.is_some() || self.waiver_by.is_some() {
                    return Err("closing_cancel_waiver_forbidden");
                }
            }
            CompanyCloseKind::Waived => {
                if !matches!(
                    self.project_status,
                    ProjectStatus::Accepted | ProjectStatus::Closed
                ) || self.acceptance_status != AcceptanceStatus::Waived
                    || !self.all_runs_stopped
                    || self.residual_obligations.is_empty()
                    || self.waiver_ref.as_deref().is_none_or(str::is_empty)
                    || self.waiver_by.as_deref().is_none_or(str::is_empty)
                {
                    return Err("closing_waiver_requirements_missing");
                }
                if self.waiver_by.as_deref() == Some(self.closer_id.as_str()) {
                    return Err("closing_waiver_self_approval_forbidden");
                }
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("closing_receipt_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt_id": self.receipt_id,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "close_kind": self.close_kind,
            "project_status": self.project_status,
            "acceptance_status": self.acceptance_status,
            "delivery_status": self.delivery_status,
            "reason": self.reason,
            "packet_attempt_refs": self.packet_attempt_refs,
            "run_refs": self.run_refs,
            "review_refs": self.review_refs,
            "acceptance_refs": self.acceptance_refs,
            "delivery_refs": self.delivery_refs,
            "incident_refs": self.incident_refs,
            "evidence_refs": self.evidence_refs,
            "residual_obligations": self.residual_obligations,
            "all_runs_stopped": self.all_runs_stopped,
            "unresolved_incidents": self.unresolved_incidents,
            "authors": self.authors,
            "reviewer_refs": self.reviewer_refs,
            "closer_id": self.closer_id,
            "waiver_ref": self.waiver_ref,
            "waiver_by": self.waiver_by,
            "closed_at": self.closed_at,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompanyClosingReceiptLedger {
    pub receipts: BTreeMap<String, CompanyClosingReceiptContract>,
}

impl CompanyClosingReceiptLedger {
    pub fn record(&mut self, receipt: CompanyClosingReceiptContract) -> Result<(), &'static str> {
        receipt.validate()?;
        if let Some(existing) = self.receipts.get(&receipt.receipt_id) {
            if existing.digest == receipt.digest {
                return Ok(());
            }
            return Err("closing_receipt_duplicate_digest_mismatch");
        }
        if self
            .receipts
            .values()
            .any(|existing| existing.project_id == receipt.project_id)
        {
            return Err("closing_receipt_project_already_closed");
        }
        self.receipts.insert(receipt.receipt_id.clone(), receipt);
        Ok(())
    }

    pub fn for_project(&self, project_id: &str) -> Option<&CompanyClosingReceiptContract> {
        self.receipts
            .values()
            .find(|receipt| receipt.project_id == project_id)
    }
}
