//! Structured Company proposal and Plan approval facts.
//!
//! Proposals are inert previews. They contain no command text interpreter and no runtime Grant or
//! Lease. Materialization remains an explicit ControlPlane transaction over the reviewed digest.
use crate::{json_digest, PlanProposal, ResultContract};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const COMPANY_PLAN_PROPOSAL_SCHEMA: &str = "kiana.company-plan-proposal.v1";
pub const COMPANY_PLAN_APPROVAL_SCHEMA: &str = "kiana.company-plan-approval.v1";

fn required(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("company_proposal_field_required_or_too_large")
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanProposal {
    pub schema: String,
    pub proposal_id: String,
    pub project_id: String,
    pub source_assignment: String,
    pub source_session: String,
    pub input_refs: Vec<String>,
    pub plan: PlanProposal,
    pub result: ResultContract,
    pub digest: String,
}

impl CompanyPlanProposal {
    pub fn new(
        proposal_id: impl Into<String>,
        project_id: impl Into<String>,
        source_assignment: impl Into<String>,
        source_session: impl Into<String>,
        input_refs: Vec<String>,
        plan: PlanProposal,
        result: ResultContract,
    ) -> Result<Self, &'static str> {
        let mut proposal = Self {
            schema: COMPANY_PLAN_PROPOSAL_SCHEMA.to_owned(),
            proposal_id: proposal_id.into(),
            project_id: project_id.into(),
            source_assignment: source_assignment.into(),
            source_session: source_session.into(),
            input_refs,
            plan,
            result,
            digest: String::new(),
        };
        proposal.digest = proposal.canonical_digest();
        proposal.validate()?;
        Ok(proposal)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PLAN_PROPOSAL_SCHEMA {
            return Err("company_proposal_schema_invalid");
        }
        for value in [
            &self.proposal_id,
            &self.project_id,
            &self.source_assignment,
            &self.source_session,
        ] {
            required(value)?;
        }
        if self.input_refs.is_empty()
            || self
                .input_refs
                .iter()
                .any(|reference| required(reference).is_err())
        {
            return Err("company_proposal_input_refs_required");
        }
        self.plan.validate()?;
        if self.plan.project_id != self.project_id {
            return Err("company_proposal_plan_project_mismatch");
        }
        self.result.validate()?;
        if self.digest != self.canonical_digest() {
            return Err("company_proposal_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "proposal_id": self.proposal_id,
            "project_id": self.project_id,
            "source_assignment": self.source_assignment,
            "source_session": self.source_session,
            "input_refs": self.input_refs,
            "plan": self.plan,
            "result": self.result,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanApproval {
    pub schema: String,
    pub approval_id: String,
    pub proposal_id: String,
    pub project_id: String,
    pub proposal_digest: String,
    pub expected_revision: u64,
    pub decision_ref: String,
    pub approver_assignment: String,
    pub approver_role: String,
    pub approved_at: u64,
    pub digest: String,
}

impl CompanyPlanApproval {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PLAN_APPROVAL_SCHEMA
            || self.expected_revision == u64::MAX
            || self.approved_at == 0
        {
            return Err("company_plan_approval_invalid");
        }
        for value in [
            &self.approval_id,
            &self.proposal_id,
            &self.project_id,
            &self.proposal_digest,
            &self.decision_ref,
            &self.approver_assignment,
            &self.approver_role,
        ] {
            required(value)?;
        }
        if self.approver_role != "sponsor" {
            return Err("company_plan_approval_sponsor_required");
        }
        if !self.proposal_digest.starts_with("sha256:") {
            return Err("company_plan_approval_proposal_digest_invalid");
        }
        if self.digest != self.canonical_digest() {
            return Err("company_plan_approval_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "approval_id": self.approval_id,
            "proposal_id": self.proposal_id,
            "project_id": self.project_id,
            "proposal_digest": self.proposal_digest,
            "expected_revision": self.expected_revision,
            "decision_ref": self.decision_ref,
            "approver_assignment": self.approver_assignment,
            "approver_role": self.approver_role,
            "approved_at": self.approved_at,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyProposalLedger {
    pub proposals: BTreeMap<String, CompanyPlanProposal>,
    pub approvals: BTreeMap<String, CompanyPlanApproval>,
}

impl CompanyProposalLedger {
    pub fn submit(&mut self, proposal: CompanyPlanProposal) -> Result<(), &'static str> {
        proposal.validate()?;
        if self.proposals.contains_key(&proposal.proposal_id) {
            return Err("company_proposal_duplicate");
        }
        self.proposals
            .insert(proposal.proposal_id.clone(), proposal);
        Ok(())
    }

    pub fn approve(&mut self, approval: CompanyPlanApproval) -> Result<(), &'static str> {
        approval.validate()?;
        let proposal = self
            .proposals
            .get(&approval.proposal_id)
            .ok_or("company_proposal_missing")?;
        if proposal.project_id != approval.project_id || proposal.digest != approval.proposal_digest
        {
            return Err("company_plan_approval_proposal_mismatch");
        }
        if self.approvals.contains_key(&approval.proposal_id) {
            return Err("company_plan_approval_duplicate");
        }
        self.approvals
            .insert(approval.proposal_id.clone(), approval);
        Ok(())
    }

    pub fn approved_plan(&self, proposal_id: &str) -> Option<&PlanProposal> {
        self.approvals
            .contains_key(proposal_id)
            .then(|| {
                self.proposals
                    .get(proposal_id)
                    .map(|proposal| &proposal.plan)
            })
            .flatten()
    }
}
