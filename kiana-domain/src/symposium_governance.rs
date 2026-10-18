//! Governance-bound symposium blackboards and decision records.
//!
//! This is a pure domain ledger for contributions. It does not convene a model, grant access or
//! approve a Company plan; those effects remain on the existing ControlPlane path. The joint
//! composition here is a visibility/decision-scope contract only: the frozen product rule in
//! `symposiums.rs` (`joint_symposium_frozen`) still refuses to convene a joint meeting.
use crate::{json_digest, DepartmentSpec, RoleSpec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const GOVERNED_SYMPOSIUM_SCHEMA: &str = "kiana.governed-symposium.v1";
pub const SYMPOSIUM_CONTRIBUTION_SCHEMA: &str = "kiana.symposium-contribution.v1";
pub const SYMPOSIUM_DECISION_SCHEMA: &str = "kiana.symposium-decision.v1";
pub const SYMPOSIUM_ASSIGNMENT_SCOPE_SCHEMA: &str = "kiana.symposium-assignment-scope.v1";

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

/// Per-assignment visibility and decision authority.
///
/// This is a contract only. It does not grant a model session, a capability or an approval; the
/// board merely refuses contributions and decisions outside the declared scope. Joint meetings are
/// frozen as a product feature (`joint_symposium_frozen`); a joint governance object can be built
/// and inspected, but no joint meeting is convened.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymposiumAssignmentScope {
    pub schema: String,
    pub assignment_ref: String,
    pub role_id: String,
    pub visible_kinds: Vec<SymposiumContributionKind>,
    pub may_decide: bool,
}

impl SymposiumAssignmentScope {
    pub fn new(
        assignment_ref: impl Into<String>,
        role_id: impl Into<String>,
        mut visible_kinds: Vec<SymposiumContributionKind>,
        may_decide: bool,
    ) -> Result<Self, &'static str> {
        visible_kinds.sort_unstable();
        let scope = Self {
            schema: SYMPOSIUM_ASSIGNMENT_SCOPE_SCHEMA.to_owned(),
            assignment_ref: assignment_ref.into(),
            role_id: role_id.into(),
            visible_kinds,
            may_decide,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SYMPOSIUM_ASSIGNMENT_SCOPE_SCHEMA {
            return Err("symposium_assignment_scope_invalid");
        }
        required(&self.assignment_ref)?;
        required(&self.role_id)?;
        if self.visible_kinds.is_empty()
            || self.visible_kinds.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("symposium_assignment_scope_invalid");
        }
        Ok(())
    }

    pub fn sees(&self, kind: SymposiumContributionKind) -> bool {
        self.visible_kinds.binary_search(&kind).is_ok()
    }

    /// A vote is both visible and admissible as a contribution under the declared scope. Vote
    /// weight stays advisory; this flag never feeds approval.
    pub fn may_vote(&self) -> bool {
        self.sees(SymposiumContributionKind::Vote)
    }
}

fn default_assignment_scopes(
    chair_assignment: &str,
    attendee_assignments: &BTreeMap<String, String>,
) -> BTreeMap<String, SymposiumAssignmentScope> {
    let all_kinds = vec![
        SymposiumContributionKind::Claim,
        SymposiumContributionKind::Vote,
        SymposiumContributionKind::Draft,
    ];
    attendee_assignments
        .iter()
        .map(|(role_id, assignment)| {
            let scope = SymposiumAssignmentScope {
                schema: SYMPOSIUM_ASSIGNMENT_SCOPE_SCHEMA.to_owned(),
                assignment_ref: assignment.clone(),
                role_id: role_id.clone(),
                visible_kinds: all_kinds.clone(),
                may_decide: assignment == chair_assignment,
            };
            (assignment.clone(), scope)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymposiumGovernanceKind {
    /// One department's own meeting. Every attendee role resolves to that department.
    #[default]
    Department,
    /// Cross-department composition contract. Every assignment must declare its own visible
    /// kinds and decision authority; the product rule still refuses to convene it.
    Joint,
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
    #[serde(default)]
    pub kind: SymposiumGovernanceKind,
    #[serde(default)]
    pub assignment_scopes: BTreeMap<String, SymposiumAssignmentScope>,
    pub digest: String,
}

impl SymposiumGovernance {
    #[allow(clippy::too_many_arguments)]
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
        let chair_assignment = chair_assignment.into();
        let scopes = default_assignment_scopes(&chair_assignment, &attendee_assignments);
        Self::with_assignment_scopes(
            symposium_id,
            project_id,
            baseline_version,
            agenda_ref,
            chair_assignment,
            attendee_assignments,
            max_rounds,
            output_schema,
            visibility,
            SymposiumGovernanceKind::Department,
            scopes,
        )
    }

    /// Build the joint composition contract. Callers must supply one explicit scope per named
    /// assignment; the default department scopes are never inferred here. The frozen product rule
    /// (`joint_symposium_frozen`) still refuses to convene a joint meeting.
    #[allow(clippy::too_many_arguments)]
    pub fn joint(
        symposium_id: impl Into<String>,
        project_id: impl Into<String>,
        baseline_version: u64,
        agenda_ref: impl Into<String>,
        chair_assignment: impl Into<String>,
        attendee_assignments: BTreeMap<String, String>,
        max_rounds: u32,
        output_schema: impl Into<String>,
        visibility: SymposiumVisibility,
        assignment_scopes: BTreeMap<String, SymposiumAssignmentScope>,
    ) -> Result<Self, &'static str> {
        Self::with_assignment_scopes(
            symposium_id,
            project_id,
            baseline_version,
            agenda_ref,
            chair_assignment,
            attendee_assignments,
            max_rounds,
            output_schema,
            visibility,
            SymposiumGovernanceKind::Joint,
            assignment_scopes,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_assignment_scopes(
        symposium_id: impl Into<String>,
        project_id: impl Into<String>,
        baseline_version: u64,
        agenda_ref: impl Into<String>,
        chair_assignment: impl Into<String>,
        attendee_assignments: BTreeMap<String, String>,
        max_rounds: u32,
        output_schema: impl Into<String>,
        visibility: SymposiumVisibility,
        kind: SymposiumGovernanceKind,
        assignment_scopes: BTreeMap<String, SymposiumAssignmentScope>,
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
            kind,
            assignment_scopes,
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
        if self.kind == SymposiumGovernanceKind::Department {
            let Some(department) = self.department() else {
                return Err("symposium_department_cannot_convene");
            };
            if !department.can_convene || department.convene_chair_id().is_none() {
                return Err("symposium_department_cannot_convene");
            }
            if !self.chair_is_department_convenor() {
                return Err("symposium_decision_chair_required");
            }
        }
        // Every named assignment needs an explicit, distinct scope; an assignment outside the
        // declared scope cannot see or decide, so an undeclared assignment fails closed.
        if self.assignment_scopes.len() != self.attendee_assignments.len() {
            return Err("symposium_assignment_scope_missing");
        }
        for (role_id, assignment) in &self.attendee_assignments {
            let Some(scope) = self.assignment_scopes.get(assignment) else {
                return Err("symposium_assignment_scope_missing");
            };
            scope.validate()?;
            if RoleSpec::lookup(&scope.role_id).is_none() {
                return Err("symposium_role_unknown");
            }
            if &scope.assignment_ref != assignment || &scope.role_id != role_id {
                return Err("symposium_assignment_scope_mismatch");
            }
        }
        let chair = self
            .assignment_scopes
            .get(&self.chair_assignment)
            .ok_or("symposium_assignment_scope_missing")?;
        if !chair.may_decide {
            return Err("symposium_decision_authority_missing");
        }
        if self.kind == SymposiumGovernanceKind::Joint {
            // A joint composition is only meaningful when decision authority is explicitly
            // bounded: at least one named assignment must be scope-limited to observe/recommend.
            let limited = self
                .assignment_scopes
                .values()
                .filter(|scope| !scope.may_decide)
                .count();
            if self.assignment_scopes.len() > 1 && limited == 0 {
                return Err("symposium_joint_scope_undeclared");
            }
            for scope in self.assignment_scopes.values() {
                if scope.may_decide
                    && !RoleSpec::lookup(&scope.role_id).is_some_and(|role| role.can_convene)
                {
                    return Err("symposium_joint_decision_authority_denied");
                }
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("symposium_governance_digest_mismatch");
        }
        Ok(())
    }

    /// The department implied by the attendee roles. `None` when roles span departments or name an
    /// unknown role, which is exactly the frozen joint composition.
    pub fn department(&self) -> Option<DepartmentSpec> {
        let mut department_id: Option<String> = None;
        for role_id in self.attendee_assignments.keys() {
            let role = RoleSpec::lookup(role_id)?;
            match &department_id {
                None => department_id = Some(role.department_id.clone()),
                Some(current) if current != &role.department_id => return None,
                Some(_) => {}
            }
        }
        DepartmentSpec::lookup(department_id.as_deref()?)
    }

    /// True when the chair is the department's `can_convene` role derived from DepartmentSpec.
    pub fn chair_is_department_convenor(&self) -> bool {
        let Some(department) = self.department() else {
            return false;
        };
        let Some(chair_role) = self.role_for_assignment(&self.chair_assignment) else {
            return false;
        };
        department.convene_chair_id().as_deref() == Some(chair_role)
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
            "kind": self.kind,
            "assignment_scopes": self.assignment_scopes,
        }))
    }

    pub fn role_for_assignment(&self, assignment: &str) -> Option<&str> {
        self.attendee_assignments
            .iter()
            .find_map(|(role, value)| (value == assignment).then_some(role.as_str()))
    }

    pub fn scope_for_assignment(&self, assignment: &str) -> Option<&SymposiumAssignmentScope> {
        self.assignment_scopes.get(assignment)
    }

    /// Assignments that may publish a decision under this contract (chair only in practice).
    pub fn decision_assignments(&self) -> Vec<&str> {
        self.assignment_scopes
            .iter()
            .filter(|(_, scope)| scope.may_decide)
            .map(|(assignment, _)| assignment.as_str())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymposiumContributionKind {
    Claim,
    Vote,
    Draft,
}

impl SymposiumContributionKind {
    /// Every kind, in declaration order; used to express "sees the whole board".
    pub const ALL: [Self; 3] = [Self::Claim, Self::Vote, Self::Draft];
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
        let Some(scope) = self
            .governance
            .scope_for_assignment(&contribution.assignment_ref)
        else {
            return Err("symposium_assignment_scope_missing");
        };
        // Per-assignment scope is enforced on record, not only on read: a role that may see a
        // kind but may not contribute it (votes are advisory) is refused here.
        if !scope.sees(contribution.kind)
            || (contribution.kind == SymposiumContributionKind::Vote && !scope.may_vote())
        {
            return Err("symposium_contribution_scope_denied");
        }
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
        let Some(scope) = self.governance.scope_for_assignment(assignment_ref) else {
            // An assignment outside the contract sees nothing, not the whole board.
            return Vec::new();
        };
        self.contributions
            .iter()
            .filter(|contribution| {
                contribution.assignment_ref == assignment_ref
                    || (contribution.visibility == SymposiumVisibility::Blackboard
                        && scope.sees(contribution.kind))
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
        let Some(scope) = self
            .governance
            .scope_for_assignment(&decision.decided_by_assignment)
        else {
            return Err("symposium_decision_authority_missing");
        };
        if !scope.may_decide {
            return Err("symposium_decision_authority_denied");
        }
        // Votes are advisory input, never approval: a decision whose only basis is votes recorded
        // on the board is refused even when the deciding assignment holds scope authority.
        if self.decision_rests_solely_on_votes(&decision) {
            return Err("symposium_vote_does_not_approve");
        }
        if self.decision.is_some() {
            return Err("symposium_decision_duplicate");
        }
        self.decision = Some(decision);
        Ok(())
    }

    /// A decision rests solely on votes when it cites at least one vote contribution, cites no
    /// non-vote contribution, and names no evidence outside the board's vote records.
    fn decision_rests_solely_on_votes(&self, decision: &SymposiumDecision) -> bool {
        let mut cited_votes = 0_usize;
        let mut cited_other = 0_usize;
        for reference in &decision.evidence_refs {
            if let Some(contribution) = self
                .contributions
                .iter()
                .find(|contribution| &contribution.contribution_id == reference)
            {
                if contribution.kind == SymposiumContributionKind::Vote {
                    cited_votes += 1;
                } else {
                    cited_other += 1;
                }
            } else {
                cited_other += 1;
            }
        }
        cited_votes > 0 && cited_other == 0
    }

    /// True when no vote contribution has been recorded against this board's baseline.
    pub fn has_votes(&self) -> bool {
        self.contributions
            .iter()
            .any(|contribution| contribution.kind == SymposiumContributionKind::Vote)
    }

    pub fn attach_sponsor_approval(
        &mut self,
        approval_ref: impl Into<String>,
        sponsor_assignment: &str,
    ) -> Result<(), &'static str> {
        let approval_ref = approval_ref.into();
        required(&approval_ref)?;
        // Sponsor approval is a separate authority: the sponsor's own declared scope must allow
        // deciding, and the company role must be the sponsor role. Vote count never substitutes.
        let Some(scope) = self.governance.scope_for_assignment(sponsor_assignment) else {
            return Err("symposium_sponsor_uninvited");
        };
        if !scope.may_decide {
            return Err("symposium_sponsor_scope_denied");
        }
        let Some(role) = self.governance.role_for_assignment(sponsor_assignment) else {
            return Err("symposium_sponsor_uninvited");
        };
        if role != "sponsor" || scope.role_id != role {
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
