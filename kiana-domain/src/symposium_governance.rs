//! Governance-bound symposium blackboards and decision records.
//!
//! This is a pure domain ledger for contributions. It does not convene a model, grant access or
//! approve a Company plan; those effects remain on the existing ControlPlane path.
use crate::{json_digest, RoleSpec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const GOVERNED_SYMPOSIUM_SCHEMA: &str = "kiana.governed-symposium.v1";
pub const SYMPOSIUM_CONTRIBUTION_SCHEMA: &str = "kiana.symposium-contribution.v1";
pub const SYMPOSIUM_DECISION_SCHEMA: &str = "kiana.symposium-decision.v1";

fn required(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("symposium_governance_field_required_or_too_large")
    } else {
        Ok(())
    }
}

fn refs(values: &[String]) -> Result<(), &'static str> {
    if values.is_empty() || values.iter().any(|value| required(value).is_err()) {
        return Err("symposium_evidence_required");
    }
    let mut seen = BTreeSet::new();
    if values.iter().any(|value| !seen.insert(value)) {
        return Err("symposium_duplicate_reference");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymposiumVisibility {
    Blackboard,
    PrivateBrief,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymposiumGovernance {
    pub schema: String,
    pub symposium_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub agenda_ref: String,
    pub chair_assignment: String,
    pub attendee_assignments: BTreeMap<String, String>,
    pub max_rounds: u32,
    pub output_schema: String,
    pub visibility: SymposiumVisibility,
    pub digest: String,
}

impl SymposiumGovernance {
    pub fn new(
        symposium_id: impl Into<String>,
        project_id: impl Into<String>,
        baseline_version: u64,
        agenda_ref: impl Into<String>,
        chair_assignment: impl Into<String>,
        attendee_assignments: BTreeMap<String, String>,
        max_rounds: u32,
        output_schema: impl Into<String>,
        visibility: SymposiumVisibility,
    ) -> Result<Self, &'static str> {
        let mut governance = Self {
            schema: GOVERNED_SYMPOSIUM_SCHEMA.to_owned(),
            symposium_id: symposium_id.into(),
            project_id: project_id.into(),
            baseline_version,
            agenda_ref: agenda_ref.into(),
            chair_assignment: chair_assignment.into(),
            attendee_assignments,
            max_rounds,
            output_schema: output_schema.into(),
            visibility,
            digest: String::new(),
        };
        governance.digest = governance.canonical_digest();
        governance.validate()?;
        Ok(governance)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != GOVERNED_SYMPOSIUM_SCHEMA
            || self.baseline_version == 0
            || self.max_rounds == 0
            || self.max_rounds > 8
        {
            return Err("symposium_governance_schema_or_limits_invalid");
        }
        for value in [
            &self.symposium_id,
            &self.project_id,
            &self.agenda_ref,
            &self.chair_assignment,
            &self.output_schema,
        ] {
            required(value)?;
        }
        if self.attendee_assignments.is_empty()
            || self.attendee_assignments.len() > 16
            || !self
                .attendee_assignments
                .values()
                .any(|assignment| assignment == &self.chair_assignment)
        {
            return Err("symposium_assignments_invalid");
        }
        let mut assignments = BTreeSet::new();
        for (role_id, assignment) in &self.attendee_assignments {
            required(role_id)?;
            required(assignment)?;
            if !assignments.insert(assignment) {
                return Err("symposium_assignment_duplicate");
            }
            if RoleSpec::lookup(role_id).is_none() {
                return Err("symposium_role_unknown");
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("symposium_governance_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "symposium_id": self.symposium_id,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "agenda_ref": self.agenda_ref,
            "chair_assignment": self.chair_assignment,
            "attendee_assignments": self.attendee_assignments,
            "max_rounds": self.max_rounds,
            "output_schema": self.output_schema,
            "visibility": self.visibility,
        }))
    }

    fn role_for_assignment(&self, assignment: &str) -> Option<&str> {
        self.attendee_assignments
            .iter()
            .find_map(|(role, value)| (value == assignment).then_some(role.as_str()))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymposiumContributionKind {
    Claim,
    Vote,
    Draft,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymposiumContribution {
    pub schema: String,
    pub contribution_id: String,
    pub symposium_id: String,
    pub assignment_ref: String,
    pub target_baseline_version: u64,
    pub kind: SymposiumContributionKind,
    pub text: String,
    pub visibility: SymposiumVisibility,
    pub evidence_refs: Vec<String>,
    pub digest: String,
}

impl SymposiumContribution {
    pub fn new(
        contribution_id: impl Into<String>,
        symposium_id: impl Into<String>,
        assignment_ref: impl Into<String>,
        target_baseline_version: u64,
        kind: SymposiumContributionKind,
        text: impl Into<String>,
        visibility: SymposiumVisibility,
        evidence_refs: Vec<String>,
    ) -> Result<Self, &'static str> {
        let mut contribution = Self {
            schema: SYMPOSIUM_CONTRIBUTION_SCHEMA.to_owned(),
            contribution_id: contribution_id.into(),
            symposium_id: symposium_id.into(),
            assignment_ref: assignment_ref.into(),
            target_baseline_version,
            kind,
            text: text.into(),
            visibility,
            evidence_refs,
            digest: String::new(),
        };
        contribution.digest = contribution.canonical_digest();
        contribution.validate()?;
        Ok(contribution)
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SYMPOSIUM_CONTRIBUTION_SCHEMA || self.target_baseline_version == 0 {
            return Err("symposium_contribution_schema_invalid");
        }
        for value in [
            &self.contribution_id,
            &self.symposium_id,
            &self.assignment_ref,
            &self.text,
        ] {
            required(value)?;
        }
        refs(&self.evidence_refs)?;
        if self.digest != self.canonical_digest() {
            return Err("symposium_contribution_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "contribution_id": self.contribution_id,
            "symposium_id": self.symposium_id,
            "assignment_ref": self.assignment_ref,
            "target_baseline_version": self.target_baseline_version,
            "kind": self.kind,
            "text": self.text,
            "visibility": self.visibility,
            "evidence_refs": self.evidence_refs,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymposiumDecision {
    pub schema: String,
    pub symposium_id: String,
    pub baseline_version: u64,
    pub summary: String,
    pub decision: String,
    pub alternatives: Vec<String>,
    pub dissent: Vec<String>,
    pub unresolved: Vec<String>,
    pub decided_by_assignment: String,
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub sponsor_approval_ref: Option<String>,
    pub digest: String,
}

impl SymposiumDecision {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SYMPOSIUM_DECISION_SCHEMA || self.baseline_version == 0 {
            return Err("symposium_decision_schema_invalid");
        }
        for value in [
            &self.symposium_id,
            &self.summary,
            &self.decision,
            &self.decided_by_assignment,
        ] {
            required(value)?;
        }
        refs(&self.evidence_refs)?;
        for value in self
            .alternatives
            .iter()
            .chain(self.dissent.iter())
            .chain(self.unresolved.iter())
        {
            required(value)?;
        }
        if let Some(reference) = &self.sponsor_approval_ref {
            required(reference)?;
        }
        if self.digest != self.canonical_digest() {
            return Err("symposium_decision_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "symposium_id": self.symposium_id,
            "baseline_version": self.baseline_version,
            "summary": self.summary,
            "decision": self.decision,
            "alternatives": self.alternatives,
            "dissent": self.dissent,
            "unresolved": self.unresolved,
            "decided_by_assignment": self.decided_by_assignment,
            "evidence_refs": self.evidence_refs,
            "sponsor_approval_ref": self.sponsor_approval_ref,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymposiumBoard {
    pub governance: SymposiumGovernance,
    #[serde(default)]
    pub contributions: Vec<SymposiumContribution>,
    #[serde(default)]
    pub decision: Option<SymposiumDecision>,
}

impl SymposiumBoard {
    pub fn new(governance: SymposiumGovernance) -> Result<Self, &'static str> {
        governance.validate()?;
        Ok(Self {
            governance,
            contributions: Vec::new(),
            decision: None,
        })
    }

    pub fn record(&mut self, contribution: SymposiumContribution) -> Result<(), &'static str> {
        contribution.validate()?;
        if contribution.symposium_id != self.governance.symposium_id {
            return Err("symposium_contribution_target_mismatch");
        }
        if contribution.target_baseline_version != self.governance.baseline_version {
            return Err("symposium_contribution_stale");
        }
        if self
            .contributions
            .iter()
            .any(|existing| existing.contribution_id == contribution.contribution_id)
        {
            return Err("symposium_contribution_duplicate");
        }
        let Some(role) = self
            .governance
            .role_for_assignment(&contribution.assignment_ref)
        else {
            return Err("symposium_contributor_uninvited");
        };
        if contribution.visibility == SymposiumVisibility::PrivateBrief
            && role
                != self
                    .governance
                    .role_for_assignment(&self.governance.chair_assignment)
                    .unwrap_or_default()
        {
            return Err("symposium_private_brief_visibility_denied");
        }
        self.contributions.push(contribution);
        Ok(())
    }

    pub fn visible_to(&self, assignment_ref: &str) -> Vec<&SymposiumContribution> {
        self.contributions
            .iter()
            .filter(|contribution| {
                contribution.visibility == SymposiumVisibility::Blackboard
                    || contribution.assignment_ref == assignment_ref
                    || assignment_ref == self.governance.chair_assignment
            })
            .collect()
    }

    pub fn publish_decision(&mut self, decision: SymposiumDecision) -> Result<(), &'static str> {
        decision.validate()?;
        if decision.symposium_id != self.governance.symposium_id
            || decision.baseline_version != self.governance.baseline_version
        {
            return Err("symposium_decision_target_mismatch");
        }
        if decision.decided_by_assignment != self.governance.chair_assignment {
            return Err("symposium_decision_chair_required");
        }
        if self.decision.is_some() {
            return Err("symposium_decision_duplicate");
        }
        self.decision = Some(decision);
        Ok(())
    }

    pub fn attach_sponsor_approval(
        &mut self,
        approval_ref: impl Into<String>,
        sponsor_assignment: &str,
    ) -> Result<(), &'static str> {
        let approval_ref = approval_ref.into();
        required(&approval_ref)?;
        let Some(role) = self.governance.role_for_assignment(sponsor_assignment) else {
            return Err("symposium_sponsor_uninvited");
        };
        if role != "sponsor" {
            return Err("symposium_sponsor_required");
        }
        let decision = self
            .decision
            .as_mut()
            .ok_or("symposium_decision_required")?;
        if decision.sponsor_approval_ref.is_some() {
            return Err("symposium_sponsor_approval_duplicate");
        }
        decision.sponsor_approval_ref = Some(approval_ref);
        decision.digest = decision.canonical_digest();
        Ok(())
    }
}
