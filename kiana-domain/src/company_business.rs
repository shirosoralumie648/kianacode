//! Versioned Company business decisions sharing the existing Company aggregate.
//! Values are durable business authority; runtime grants remain ControlPlane-owned.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_BUSINESS_SCHEMA: &str = "kiana.company-business.v2";
type Result<T> = std::result::Result<T, &'static str>;
fn check(ok: bool, reason: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(reason)
    }
}
fn text(value: &str) -> Result<()> {
    check(
        !value.trim().is_empty() && value.len() <= 16384,
        "business_field_invalid",
    )
}
fn refs(values: &[String]) -> Result<()> {
    check(
        !values.is_empty() && values.len() <= 1024,
        "business_references_required",
    )?;
    for value in values {
        text(value)?;
    }
    check(
        values.iter().collect::<BTreeSet<_>>().len() == values.len(),
        "business_duplicate_reference",
    )
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessOrganization {
    pub organization_id: String,
    pub name: String,
    pub owner_principal_id: String,
    pub workspace: String,
    pub version: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessActorKind {
    Human,
    Agent,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessAssignment {
    pub assignment_id: String,
    pub organization_id: String,
    pub owner_principal_id: String,
    pub role_id: String,
    pub actor_kind: BusinessActorKind,
    pub project_ids: Vec<String>,
    pub expires_at: u64,
    pub version: u64,
    pub revoked: bool,
}
impl BusinessAssignment {
    pub fn active(&self, actor: &str, role: &str, project: &str, now: u64) -> bool {
        self.owner_principal_id == actor
            && self.role_id == role
            && self.project_ids.iter().any(|id| id == project)
            && !self.revoked
            && self.expires_at > now
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AcceptanceTarget {
    Packet(String),
    Milestone(String),
    Project(String),
}
impl AcceptanceTarget {
    pub fn key(&self) -> String {
        match self {
            Self::Packet(id) => format!("packet:{id}"),
            Self::Milestone(id) => format!("milestone:{id}"),
            Self::Project(id) => format!("project:{id}"),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessCriterion {
    pub criterion_id: String,
    pub target: AcceptanceTarget,
    pub description: String,
    pub required: bool,
    pub refines: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessStakeholder {
    pub principal_id: String,
    pub responsibility: String,
    pub delivery_recipient: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessBaseline {
    pub project_id: String,
    pub organization_id: String,
    pub workspace: String,
    pub version: u64,
    pub charter_ref: String,
    pub scope: String,
    pub criteria: BTreeMap<String, BusinessCriterion>,
    pub stakeholders: Vec<BusinessStakeholder>,
    pub approved_by: String,
    pub approved_at: u64,
    pub plan_version: u64,
    pub active_packets: Vec<String>,
    pub superseded_packets: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessAuthor {
    pub principal_id: String,
    pub human_owner: String,
    pub role_instance_id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub role_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessArtifact {
    pub artifact_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub content_hash: String,
    pub schema: String,
    pub producer_runs: Vec<RunId>,
    pub registered_at: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub bundle_id: String,
    pub project_id: String,
    pub packet_id: String,
    pub baseline_version: u64,
    pub packet_version: u64,
    pub run_id: RunId,
    pub authors: Vec<BusinessAuthor>,
    pub artifact_refs: Vec<String>,
    pub event_refs: Vec<String>,
    pub digest: String,
    pub created_at: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionVerdict {
    Pass,
    Fail,
    InsufficientEvidence,
    NotApplicable,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionConclusion {
    pub verdict: CriterionVerdict,
    pub evidence_refs: Vec<String>,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessReview {
    pub review_id: String,
    pub assignment_id: String,
    pub reviewer: BusinessAuthor,
    pub results: BTreeMap<String, CriterionConclusion>,
    pub evidence_digest: String,
    pub recorded_at: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessAcceptance {
    pub acceptance_id: String,
    pub project_id: String,
    pub target: AcceptanceTarget,
    pub baseline_version: u64,
    pub criteria: BTreeMap<String, BusinessCriterion>,
    pub bundle_ids: Vec<String>,
    pub authors: Vec<BusinessAuthor>,
    pub evidence_digest: String,
    pub review: Option<BusinessReview>,
    pub status: AcceptanceStatus,
    pub decision_maker: Option<String>,
    pub reasons: Vec<String>,
    pub waiver_refs: Vec<String>,
    pub created_at: u64,
    pub decided_at: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessRunBinding {
    pub packet_id: String,
    pub assignment_id: String,
    pub assignment_version: u64,
    pub baseline_version: u64,
    pub role_instance_id: String,
    pub execution_request_id: RequestId,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BusinessProof {
    pub workspace: String,
    pub actor_is_human: bool,
    pub authors: BTreeMap<String, BusinessAuthor>,
    pub runs: BTreeMap<String, CompanyRun>,
    pub reviewer: Option<BusinessAuthor>,
    pub verified_events: Vec<String>,
    pub review_results: BTreeMap<String, CriterionConclusion>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyBusinessState {
    #[serde(default)]
    pub clock_ms: u64,
    pub review_tasks: BTreeMap<String, BusinessReviewTask>,
    pub organizations: BTreeMap<String, BusinessOrganization>,
    pub assignments: BTreeMap<String, BusinessAssignment>,
    pub baselines: BTreeMap<String, BusinessBaseline>,
    pub artifacts: BTreeMap<String, BusinessArtifact>,
    pub bundles: BTreeMap<String, EvidenceBundle>,
    pub acceptances: BTreeMap<String, BusinessAcceptance>,
    pub run_bindings: BTreeMap<String, BusinessRunBinding>,
    pub paused_from: BTreeMap<String, ProjectStatus>,
    #[serde(default)]
    pub closeout: CompanyCloseoutState,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusinessReviewTask {
    pub task_id: String,
    pub acceptance_id: String,
    pub assignment_id: String,
    pub assignment_version: u64,
    pub execution_request_id: RequestId,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub evidence_digest: String,
    pub completed: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessPlannedPacket {
    pub milestone_id: String,
    pub packet: WorkPacket,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompanyBusinessAction {
    Closeout {
        schema: String,
        action: Box<BusinessCloseoutAction>,
    },
    CreateOrganization {
        organization_id: String,
        name: String,
    },
    Appoint {
        assignment: BusinessAssignment,
    },
    RevokeAssignment {
        assignment_id: String,
        reason: String,
    },
    ApproveCharter {
        project_id: String,
        organization_id: String,
        criteria: Vec<BusinessCriterion>,
        stakeholders: Vec<BusinessStakeholder>,
        budget: CompanyBudgetPolicy,
        decision_ref: String,
    },
    PublishPlan {
        project_id: String,
        baseline_version: u64,
        milestones: Vec<Milestone>,
        packets: Vec<BusinessPlannedPacket>,
        criteria: Vec<BusinessCriterion>,
        decision_ref: String,
    },
    RegisterEvidence {
        project_id: String,
        artifact_id: String,
        relative_path: String,
        schema: String,
        producer_runs: Vec<RunId>,
    },
    CollectEvidence {
        bundle_id: String,
        packet_id: String,
        artifact_refs: Vec<String>,
    },
    RequestAcceptance {
        acceptance_id: String,
        target: AcceptanceTarget,
        bundle_ids: Vec<String>,
    },
    StartReview {
        acceptance_id: String,
        assignment_id: String,
        task_id: String,
    },
    ReconcileReview {
        task_id: String,
    },
    RecordReview {
        acceptance_id: String,
        review_id: String,
        assignment_id: String,
        reviewer_run_id: Option<RunId>,
        results: BTreeMap<String, CriterionConclusion>,
    },
    DecideAcceptance {
        acceptance_id: String,
        decision: AcceptanceDecision,
        reasons: Vec<String>,
        waiver_refs: Vec<String>,
    },
    Rework {
        packet_id: String,
        replacement: WorkPacket,
        reason: String,
        max_attempts: u32,
    },
    Pause {
        project_id: String,
    },
    Resume {
        project_id: String,
    },
}
impl CompanyBusinessAction {
    pub fn roles(&self) -> &'static [&'static str] {
        match self {
            Self::Closeout { action, .. } => action.roles(),
            Self::CreateOrganization { .. }
            | Self::Appoint { .. }
            | Self::RevokeAssignment { .. }
            | Self::ApproveCharter { .. } => &["sponsor"],
            Self::PublishPlan { .. } | Self::Rework { .. } => &["pm"],
            Self::RecordReview { .. } | Self::StartReview { .. } | Self::ReconcileReview { .. } => {
                &["reviewer"]
            }
            Self::DecideAcceptance { .. } => &["sponsor", "reviewer"],
            Self::RegisterEvidence { .. } | Self::CollectEvidence { .. } => {
                &["builder", "reviewer", "architect", "pm", "closer"]
            }
            Self::RequestAcceptance { .. } => &["builder", "closer", "pm"],
            Self::Pause { .. } | Self::Resume { .. } => &["sponsor", "pm"],
        }
    }
    pub fn project_id(&self, state: &CompanyState) -> Option<String> {
        match self {
            Self::Closeout { action, .. } => action.project_id(state),
            Self::ApproveCharter { project_id, .. }
            | Self::PublishPlan { project_id, .. }
            | Self::RegisterEvidence { project_id, .. }
            | Self::Pause { project_id }
            | Self::Resume { project_id } => Some(project_id.clone()),
            Self::CollectEvidence { packet_id, .. } | Self::Rework { packet_id, .. } => {
                state.packets.get(packet_id).map(|p| p.project_id.clone())
            }
            Self::RequestAcceptance { target, .. } => state.business_target_project(target).ok(),
            Self::StartReview { acceptance_id, .. }
            | Self::RecordReview { acceptance_id, .. }
            | Self::DecideAcceptance { acceptance_id, .. } => state
                .business
                .acceptances
                .get(acceptance_id)
                .map(|a| a.project_id.clone()),
            Self::ReconcileReview { task_id } => state
                .business
                .review_tasks
                .get(task_id)
                .and_then(|task| state.business.acceptances.get(&task.acceptance_id))
                .map(|a| a.project_id.clone()),
            _ => None,
        }
    }
    pub fn references(&self) -> Vec<String> {
        match self {
            Self::Closeout { action, .. } => action.references(),
            Self::ApproveCharter { decision_ref, .. } | Self::PublishPlan { decision_ref, .. } => {
                vec![decision_ref.clone()]
            }
            Self::CollectEvidence { artifact_refs, .. } => artifact_refs.clone(),
            Self::RecordReview { results, .. } => results
                .values()
                .flat_map(|r| r.evidence_refs.clone())
                .collect(),
            Self::DecideAcceptance { waiver_refs, .. } => waiver_refs.clone(),
            _ => vec![],
        }
    }
}
impl CompanyState {
    pub fn business_target_project(&self, target: &AcceptanceTarget) -> Result<String> {
        Ok(match target {
            AcceptanceTarget::Project(id) => {
                check(self.projects.contains_key(id), "business_project_missing")?;
                id.clone()
            }
            AcceptanceTarget::Milestone(id) => self
                .milestones
                .get(id)
                .ok_or("business_milestone_missing")?
                .project_id
                .clone(),
            AcceptanceTarget::Packet(id) => self
                .packets
                .get(id)
                .ok_or("business_packet_missing")?
                .project_id
                .clone(),
        })
    }
    pub fn business_assignment(
        &self,
        actor: &str,
        role: &str,
        project: &str,
        now: u64,
    ) -> Result<&BusinessAssignment> {
        let mut matches = self
            .business
            .assignments
            .values()
            .filter(|a| a.active(actor, role, project, now));
        let first = matches.next().ok_or("business_assignment_required")?;
        check(matches.next().is_none(), "business_assignment_ambiguous")?;
        Ok(first)
    }
    pub fn business_accepted(&self, target: &AcceptanceTarget, baseline: u64) -> bool {
        self.business.acceptances.values().any(|a| {
            a.target == *target
                && a.baseline_version == baseline
                && a.status == AcceptanceStatus::Accepted
        })
    }
    pub fn business_packet_blockers(&self, packet_id: &str, now: u64) -> Vec<String> {
        let Some(packet) = self.packets.get(packet_id) else {
            return vec!["packet_missing".into()];
        };
        let Some(baseline) = self.business.baselines.get(&packet.project_id) else {
            return vec![];
        };
        let mut blocked = Vec::new();
        if !baseline.active_packets.contains(&packet_id.to_string()) {
            blocked.push("packet_not_in_current_plan".into());
        }
        if self
            .projects
            .get(&packet.project_id)
            .is_none_or(|p| !matches!(p.status, ProjectStatus::Planned | ProjectStatus::Active))
        {
            blocked.push("project_not_active".into());
        }
        for dependency in &packet.packet.dependencies {
            let effective = baseline
                .superseded_packets
                .get(dependency)
                .unwrap_or(dependency);
            if !self.business_accepted(
                &AcceptanceTarget::Packet(effective.clone()),
                baseline.version,
            ) {
                blocked.push(format!("packet_acceptance_required:{effective}"));
            }
        }
        if let Some(milestone) = self.milestones.get(&packet.milestone_id) {
            for dependency in &milestone.dependency_refs {
                if !self.business_accepted(
                    &AcceptanceTarget::Milestone(dependency.clone()),
                    baseline.version,
                ) {
                    blocked.push(format!("milestone_acceptance_required:{dependency}"));
                }
            }
        }
        if let Some(binding) = self.business.run_bindings.get(packet_id) {
            if self
                .business
                .assignments
                .get(&binding.assignment_id)
                .is_none_or(|a| {
                    a.revoked || a.version != binding.assignment_version || a.expires_at <= now
                })
            {
                blocked.push("assignment_revoked_or_expired".into());
            }
            if binding.baseline_version != baseline.version {
                blocked.push("run_baseline_stale".into());
            }
        }
        blocked
    }
    pub(crate) fn business_legacy_guard(
        &self,
        c: &CompanyCommand,
        a: &CompanyAuthority,
    ) -> Result<()> {
        let project = match c {
            CompanyCommand::StartRun { project_id, .. }
            | CompanyCommand::CreateMilestone {
                milestone: Milestone { project_id, .. },
            }
            | CompanyCommand::ApprovePacket { project_id, .. }
            | CompanyCommand::PlanProject { project_id }
            | CompanyCommand::RequestAcceptance { project_id, .. }
            | CompanyCommand::ApproveProject { project_id, .. }
            | CompanyCommand::PauseProject { project_id }
            | CompanyCommand::ResumeProject { project_id }
            | CompanyCommand::CloseProject { project_id, .. }
            | CompanyCommand::RecordOutcome { project_id, .. } => Some(project_id.as_str()),
            CompanyCommand::ReviewPacket { packet_id, .. }
            | CompanyCommand::ReworkPacket { packet_id, .. } => {
                self.packets.get(packet_id).map(|p| p.project_id.as_str())
            }
            CompanyCommand::RecordReview { acceptance_id, .. }
            | CompanyCommand::DecideAcceptance { acceptance_id, .. } => self
                .acceptances
                .get(acceptance_id)
                .map(|a| a.project_id.as_str()),
            CompanyCommand::PrepareDelivery { delivery } => Some(delivery.project_id.as_str()),
            CompanyCommand::RequestChange { change } => Some(change.project_id.as_str()),
            CompanyCommand::DecideChange { change_id, .. } => {
                self.changes.get(change_id).map(|c| c.project_id.as_str())
            }
            _ => None,
        };
        if let Some(project) = project.filter(|id| self.business.baselines.contains_key(*id)) {
            if let CompanyCommand::StartRun { packet_id, .. } = c {
                self.business_assignment(&a.actor_id, "builder", project, a.now_ms)?;
                check(
                    self.business_packet_blockers(packet_id, a.now_ms)
                        .is_empty(),
                    "business_packet_blocked",
                )?;
            } else {
                return Err("business_versioned_command_required");
            }
        }
        Ok(())
    }
    pub(crate) fn apply_business(
        &mut self,
        c: &CompanyBusinessAction,
        a: &CompanyAuthority,
        p: &CompanyProof,
    ) -> Result<()> {
        check(
            a.now_ms >= self.business.clock_ms,
            "business_clock_rollback",
        )?;
        check(
            c.roles().contains(&a.role_id.as_str()),
            "business_role_denied",
        )?;
        if let Some(id) = c.project_id(self) {
            if self.business.baselines.contains_key(&id) {
                self.business_assignment(&a.actor_id, &a.role_id, &id, a.now_ms)?;
            }
        }
        match c {
            CompanyBusinessAction::Closeout { schema, action } => {
                check(schema == COMPANY_BUSINESS_SCHEMA, "business_schema_invalid")?;
                self.apply_closeout(action, a, p)?;
            }
            CompanyBusinessAction::CreateOrganization {
                organization_id,
                name,
            } => {
                check(
                    p.business.actor_is_human,
                    "business_human_decision_required",
                )?;
                text(name)?;
                check(
                    uuid::Uuid::parse_str(organization_id).is_ok(),
                    "business_organization_id_invalid",
                )?;
                check(
                    !self.business.organizations.contains_key(organization_id)
                        && self.business.organizations.len() < 64,
                    "business_organization_exists_or_limit",
                )?;
                self.business.organizations.insert(
                    organization_id.clone(),
                    BusinessOrganization {
                        organization_id: organization_id.clone(),
                        name: name.clone(),
                        owner_principal_id: a.actor_id.clone(),
                        workspace: p.business.workspace.clone(),
                        version: 1,
                    },
                );
            }
            CompanyBusinessAction::Appoint { assignment } => {
                check(
                    p.business.actor_is_human,
                    "business_human_decision_required",
                )?;
                let org = self
                    .business
                    .organizations
                    .get(&assignment.organization_id)
                    .ok_or("business_organization_missing")?;
                check(
                    org.owner_principal_id == a.actor_id && org.workspace == p.business.workspace,
                    "business_organization_scope_denied",
                )?;
                text(&assignment.assignment_id)?;
                refs(&assignment.project_ids)?;
                check(
                    assignment.owner_principal_id == a.actor_id
                        && assignment.version == 1
                        && !assignment.revoked
                        && assignment.expires_at > a.now_ms
                        && RoleSpec::lookup(&assignment.role_id).is_some(),
                    "business_assignment_invalid",
                )?;
                check(
                    !self
                        .business
                        .assignments
                        .contains_key(&assignment.assignment_id)
                        && self.business.assignments.len() < 4096,
                    "business_assignment_exists_or_limit",
                )?;
                for id in &assignment.project_ids {
                    let project = self.projects.get(id).ok_or("business_project_missing")?;
                    check(
                        project.organization_id == assignment.organization_id,
                        "business_assignment_project_mismatch",
                    )?;
                }
                self.business
                    .assignments
                    .insert(assignment.assignment_id.clone(), assignment.clone());
            }
            CompanyBusinessAction::RevokeAssignment {
                assignment_id,
                reason,
            } => {
                check(
                    p.business.actor_is_human,
                    "business_human_decision_required",
                )?;
                text(reason)?;
                let assignment = self
                    .business
                    .assignments
                    .get_mut(assignment_id)
                    .ok_or("business_assignment_missing")?;
                check(
                    assignment.owner_principal_id == a.actor_id && !assignment.revoked,
                    "business_assignment_owner_or_state_invalid",
                )?;
                assignment.revoked = true;
                assignment.version = assignment
                    .version
                    .checked_add(1)
                    .ok_or("business_version_exhausted")?;
            }
            CompanyBusinessAction::ApproveCharter {
                project_id,
                organization_id,
                criteria,
                stakeholders,
                budget,
                decision_ref,
            } => {
                check(
                    p.business.actor_is_human,
                    "business_human_decision_required",
                )?;
                let project = self
                    .projects
                    .get(project_id)
                    .ok_or("business_project_missing")?
                    .clone();
                self.business_assignment(&a.actor_id, "sponsor", project_id, a.now_ms)?;
                check(
                    project.project_budget_ref == format!("budget:{project_id}"),
                    "business_budget_reference_invalid",
                )?;
                let org = self
                    .business
                    .organizations
                    .get(organization_id)
                    .ok_or("business_organization_missing")?;
                check(
                    org.owner_principal_id == a.actor_id
                        && org.workspace == p.business.workspace
                        && project.organization_id == *organization_id,
                    "business_organization_scope_denied",
                )?;
                check(
                    !self.business.baselines.contains_key(project_id),
                    "business_baseline_exists",
                )?;
                check(
                    !stakeholders.is_empty() && stakeholders.iter().any(|s| s.delivery_recipient),
                    "business_stakeholders_required",
                )?;
                for stakeholder in stakeholders {
                    text(&stakeholder.principal_id)?;
                    text(&stakeholder.responsibility)?;
                    check(
                        stakeholder.principal_id == a.actor_id,
                        "business_external_principal_unsupported",
                    )?;
                }
                let target = AcceptanceTarget::Project(project_id.clone());
                let criteria = validate_criteria(criteria, &target)?;
                self.apply(
                    &CompanyCommand::ApproveProject {
                        project_id: project_id.clone(),
                        decision_ref: decision_ref.clone(),
                    },
                    a,
                    p,
                )?;
                self.apply(
                    &CompanyCommand::ConfigureBudget {
                        project_id: project_id.clone(),
                        policy: budget.clone(),
                    },
                    a,
                    p,
                )?;
                self.business.baselines.insert(
                    project_id.clone(),
                    BusinessBaseline {
                        project_id: project_id.clone(),
                        organization_id: organization_id.clone(),
                        workspace: p.business.workspace.clone(),
                        version: 1,
                        charter_ref: project.charter_ref,
                        scope: project.scope_baseline,
                        criteria,
                        stakeholders: stakeholders.clone(),
                        approved_by: a.actor_id.clone(),
                        approved_at: a.now_ms,
                        plan_version: 0,
                        active_packets: vec![],
                        superseded_packets: BTreeMap::new(),
                    },
                );
            }
            CompanyBusinessAction::PublishPlan {
                project_id,
                baseline_version,
                milestones,
                packets,
                criteria,
                decision_ref,
            } => {
                check(
                    p.events.contains(decision_ref),
                    "business_plan_decision_unverified",
                )?;
                let baseline = self
                    .business
                    .baselines
                    .get(project_id)
                    .ok_or("business_baseline_missing")?
                    .clone();
                check(
                    baseline.version == *baseline_version && baseline.plan_version == 0,
                    "business_plan_baseline_stale",
                )?;
                check(
                    !milestones.is_empty()
                        && milestones.len() <= 128
                        && !packets.is_empty()
                        && packets.len() <= 1024,
                    "business_plan_size_invalid",
                )?;
                let mut pending = milestones.clone();
                let mut inserted = BTreeSet::new();
                while !pending.is_empty() {
                    let position = pending
                        .iter()
                        .position(|m| m.dependency_refs.iter().all(|id| inserted.contains(id)))
                        .ok_or("business_milestone_cycle_or_missing")?;
                    let milestone = pending.remove(position);
                    check(
                        milestone.project_id == *project_id,
                        "business_cross_project_plan",
                    )?;
                    inserted.insert(milestone.milestone_id.clone());
                    self.apply(&CompanyCommand::CreateMilestone { milestone }, a, p)?;
                }
                let mut pending = packets.clone();
                let mut inserted = BTreeSet::new();
                while !pending.is_empty() {
                    let position = pending
                        .iter()
                        .position(|p| p.packet.dependencies.iter().all(|id| inserted.contains(id)))
                        .ok_or("business_packet_cycle_or_missing")?;
                    let planned = pending.remove(position);
                    inserted.insert(planned.packet.id.clone());
                    self.apply(
                        &CompanyCommand::ApprovePacket {
                            project_id: project_id.clone(),
                            milestone_id: planned.milestone_id,
                            packet: planned.packet,
                        },
                        a,
                        p,
                    )?;
                }
                let mut all = baseline.criteria.clone();
                for criterion in criteria {
                    text(&criterion.criterion_id)?;
                    text(&criterion.description)?;
                    check(
                        self.business_target_project(&criterion.target)? == *project_id
                            && !all.contains_key(&criterion.criterion_id),
                        "business_criterion_identity_invalid",
                    )?;
                    check(
                        !matches!(criterion.target, AcceptanceTarget::Project(_)),
                        "business_plan_cannot_replace_charter_criteria",
                    )?;
                    all.insert(criterion.criterion_id.clone(), criterion.clone());
                }
                for criterion in all.values() {
                    if !matches!(criterion.target, AcceptanceTarget::Project(_)) {
                        refs(&criterion.refines)?;
                    }
                    for parent in &criterion.refines {
                        let parent = all.get(parent).ok_or("business_criterion_parent_missing")?;
                        let valid = match (&criterion.target, &parent.target) {
                            (
                                AcceptanceTarget::Milestone(m),
                                AcceptanceTarget::Project(project),
                            ) => self
                                .milestones
                                .get(m)
                                .is_some_and(|m| m.project_id == *project),
                            (AcceptanceTarget::Packet(packet), AcceptanceTarget::Milestone(m)) => {
                                self.packets
                                    .get(packet)
                                    .is_some_and(|p| p.milestone_id == *m)
                            }
                            _ => false,
                        };
                        check(valid, "business_criterion_refinement_invalid")?;
                    }
                    if criterion.required
                        && !matches!(criterion.target, AcceptanceTarget::Packet(_))
                    {
                        check(
                            all.values().any(|child| {
                                child.required && child.refines.contains(&criterion.criterion_id)
                            }),
                            "business_criterion_uncovered",
                        )?;
                    }
                }
                for packet in packets {
                    check(
                        all.values().any(|c| {
                            c.target == AcceptanceTarget::Packet(packet.packet.id.clone())
                                && c.required
                        }),
                        "business_packet_criteria_missing",
                    )?;
                }
                for milestone in milestones {
                    check(
                        packets
                            .iter()
                            .any(|p| p.milestone_id == milestone.milestone_id),
                        "business_milestone_packet_missing",
                    )?;
                }
                self.apply(
                    &CompanyCommand::PlanProject {
                        project_id: project_id.clone(),
                    },
                    a,
                    p,
                )?;
                let baseline = self.business.baselines.get_mut(project_id).unwrap();
                baseline.criteria = all;
                baseline.plan_version = 1;
                baseline.active_packets = packets.iter().map(|p| p.packet.id.clone()).collect();
            }
            CompanyBusinessAction::RegisterEvidence {
                project_id,
                artifact_id,
                relative_path,
                schema,
                producer_runs,
            } => {
                text(schema)?;
                let baseline = self
                    .business
                    .baselines
                    .get(project_id)
                    .ok_or("business_baseline_missing")?;
                check(
                    !self.business.artifacts.contains_key(artifact_id),
                    "business_artifact_exists",
                )?;
                check(
                    !producer_runs.is_empty()
                        && producer_runs.iter().all(|run| {
                            p.business.authors.contains_key(&run.to_string())
                                && p.business
                                    .runs
                                    .values()
                                    .any(|r| r.run_id == Some(*run) && r.project_id == *project_id)
                        }),
                    "business_artifact_producer_unverified",
                )?;
                let artifact = p
                    .artifact
                    .clone()
                    .ok_or("business_artifact_content_required")?;
                check(
                    artifact.artifact_id == *artifact_id
                        && artifact.relative_path == *relative_path,
                    "business_artifact_content_mismatch",
                )?;
                let metadata = BusinessArtifact {
                    artifact_id: artifact_id.clone(),
                    project_id: project_id.clone(),
                    baseline_version: baseline.version,
                    content_hash: journal_sha256(artifact.text.as_bytes()),
                    schema: schema.clone(),
                    producer_runs: producer_runs.clone(),
                    registered_at: a.now_ms,
                };
                check(
                    !self.artifacts.contains_key(artifact_id),
                    "company_artifact_already_exists",
                )?;
                self.artifacts.insert(artifact_id.clone(), artifact);
                self.business
                    .artifacts
                    .insert(artifact_id.clone(), metadata);
            }
            CompanyBusinessAction::CollectEvidence {
                bundle_id,
                packet_id,
                artifact_refs,
            } => {
                text(bundle_id)?;
                refs(artifact_refs)?;
                check(
                    !self.business.bundles.contains_key(bundle_id),
                    "business_evidence_bundle_exists",
                )?;
                let packet = self
                    .packets
                    .get(packet_id)
                    .ok_or("business_packet_missing")?;
                let baseline = self
                    .business
                    .baselines
                    .get(&packet.project_id)
                    .ok_or("business_baseline_missing")?;
                let run = p
                    .business
                    .runs
                    .get(packet_id)
                    .ok_or("business_run_observation_required")?;
                check(
                    run.status == ExecutionStatus::Completed,
                    "business_run_not_completed",
                )?;
                let run_id = run.run_id.ok_or("business_run_id_missing")?;
                let mut authors = BTreeMap::new();
                for reference in artifact_refs {
                    let id = reference
                        .strip_prefix("artifact:")
                        .ok_or("business_artifact_reference_required")?;
                    let artifact = self
                        .business
                        .artifacts
                        .get(id)
                        .ok_or("business_artifact_missing")?;
                    check(
                        artifact.project_id == packet.project_id
                            && artifact.baseline_version == baseline.version
                            && artifact.producer_runs.contains(&run_id),
                        "business_artifact_scope_or_producer_mismatch",
                    )?;
                    for id in &artifact.producer_runs {
                        let author = p
                            .business
                            .authors
                            .get(&id.to_string())
                            .ok_or("business_author_proof_missing")?;
                        authors.insert(author.role_instance_id.clone(), author.clone());
                    }
                }
                check(
                    !run.evidence_refs.is_empty(),
                    "business_run_evidence_missing",
                )?;
                let authors = authors.into_values().collect::<Vec<_>>();
                let digest = json_digest(
                    &serde_json::json!({"packet":packet,"baseline":baseline.version,"run":run,"artifacts":artifact_refs,"authors":authors}),
                );
                self.business.bundles.insert(
                    bundle_id.clone(),
                    EvidenceBundle {
                        bundle_id: bundle_id.clone(),
                        project_id: packet.project_id.clone(),
                        packet_id: packet_id.clone(),
                        baseline_version: baseline.version,
                        packet_version: packet.version,
                        run_id,
                        authors,
                        artifact_refs: artifact_refs.clone(),
                        event_refs: run.evidence_refs.clone(),
                        digest,
                        created_at: a.now_ms,
                    },
                );
                self.runs.insert(packet_id.clone(), run.clone());
            }
            CompanyBusinessAction::RequestAcceptance {
                acceptance_id,
                target,
                bundle_ids,
            } => {
                text(acceptance_id)?;
                refs(bundle_ids)?;
                check(
                    !self.business.acceptances.contains_key(acceptance_id),
                    "business_acceptance_exists",
                )?;
                let project_id = self.business_target_project(target)?;
                let baseline = self
                    .business
                    .baselines
                    .get(&project_id)
                    .ok_or("business_baseline_missing")?
                    .clone();
                check(
                    self.projects.get(&project_id).is_some_and(|p| {
                        matches!(
                            p.status,
                            ProjectStatus::Active | ProjectStatus::ReadyForAcceptance
                        )
                    }),
                    "business_project_not_active",
                )?;
                check(
                    !self.business.acceptances.values().any(|existing| {
                        existing.target == *target
                            && existing.baseline_version == baseline.version
                            && !matches!(
                                existing.status,
                                AcceptanceStatus::Rejected | AcceptanceStatus::Closed
                            )
                    }),
                    "business_target_acceptance_exists",
                )?;
                let required_packets: Vec<String> = match target {
                    AcceptanceTarget::Packet(id) => vec![id.clone()],
                    AcceptanceTarget::Milestone(id) => baseline
                        .active_packets
                        .iter()
                        .filter(|packet| {
                            self.packets
                                .get(*packet)
                                .is_some_and(|p| p.milestone_id == *id)
                        })
                        .cloned()
                        .collect(),
                    AcceptanceTarget::Project(_) => baseline.active_packets.clone(),
                };
                check(
                    !required_packets.is_empty(),
                    "business_acceptance_empty_target",
                )?;
                if !matches!(target, AcceptanceTarget::Packet(_)) {
                    for id in &required_packets {
                        check(
                            self.business_accepted(
                                &AcceptanceTarget::Packet(id.clone()),
                                baseline.version,
                            ),
                            "business_packet_acceptance_required",
                        )?;
                    }
                }
                if matches!(target, AcceptanceTarget::Project(_)) {
                    for id in &self.projects[&project_id].milestone_refs {
                        check(
                            self.business_accepted(
                                &AcceptanceTarget::Milestone(id.clone()),
                                baseline.version,
                            ),
                            "business_milestone_acceptance_required",
                        )?;
                    }
                }
                let mut authors = BTreeMap::new();
                let mut covered = BTreeSet::new();
                let mut digests = Vec::new();
                for id in bundle_ids {
                    let bundle = self
                        .business
                        .bundles
                        .get(id)
                        .ok_or("business_evidence_bundle_missing")?;
                    check(
                        bundle.project_id == project_id
                            && bundle.baseline_version == baseline.version
                            && required_packets.contains(&bundle.packet_id)
                            && self
                                .packets
                                .get(&bundle.packet_id)
                                .is_some_and(|p| p.version == bundle.packet_version),
                        "business_evidence_target_or_version_mismatch",
                    )?;
                    covered.insert(bundle.packet_id.clone());
                    digests.push(bundle.digest.clone());
                    for author in &bundle.authors {
                        authors.insert(author.role_instance_id.clone(), author.clone());
                    }
                }
                check(
                    required_packets.iter().all(|id| covered.contains(id)),
                    "business_evidence_coverage_missing",
                )?;
                let criteria = baseline
                    .criteria
                    .values()
                    .filter(|criterion| criterion.target == *target)
                    .map(|c| (c.criterion_id.clone(), c.clone()))
                    .collect::<BTreeMap<_, _>>();
                check(
                    !criteria.is_empty() && criteria.values().any(|c| c.required),
                    "business_target_criteria_missing",
                )?;
                let digest = json_digest(
                    &serde_json::json!({"target":target,"baseline":baseline.version,"bundles":digests,"criteria":criteria}),
                );
                self.business.acceptances.insert(
                    acceptance_id.clone(),
                    BusinessAcceptance {
                        acceptance_id: acceptance_id.clone(),
                        project_id: project_id.clone(),
                        target: target.clone(),
                        baseline_version: baseline.version,
                        criteria,
                        bundle_ids: bundle_ids.clone(),
                        authors: authors.into_values().collect(),
                        evidence_digest: digest,
                        review: None,
                        status: AcceptanceStatus::Requested,
                        decision_maker: None,
                        reasons: vec![],
                        waiver_refs: vec![],
                        created_at: a.now_ms,
                        decided_at: None,
                    },
                );
                match target {
                    AcceptanceTarget::Milestone(id) => {
                        let m = self.milestones.get_mut(id).unwrap();
                        check(
                            matches!(m.status, MilestoneStatus::Active | MilestoneStatus::Rework),
                            "business_milestone_not_active",
                        )?;
                        m.status = MilestoneStatus::ReadyForAcceptance;
                        m.version += 1;
                    }
                    AcceptanceTarget::Project(_) => {
                        let project = self.projects.get_mut(&project_id).unwrap();
                        project.status = ProjectStatus::ReadyForAcceptance;
                        project.version += 1;
                    }
                    _ => {}
                }
            }
            CompanyBusinessAction::StartReview {
                acceptance_id,
                assignment_id,
                task_id,
            } => {
                text(task_id)?;
                check(
                    !self.business.review_tasks.contains_key(task_id),
                    "business_review_task_exists",
                )?;
                let acceptance = self
                    .business
                    .acceptances
                    .get(acceptance_id)
                    .ok_or("business_acceptance_missing")?;
                check(
                    acceptance.status == AcceptanceStatus::Requested,
                    "business_acceptance_not_reviewable",
                )?;
                let assignment = self
                    .business
                    .assignments
                    .get(assignment_id)
                    .ok_or("business_assignment_missing")?;
                check(
                    assignment.active(&a.actor_id, "reviewer", &acceptance.project_id, a.now_ms)
                        && assignment.actor_kind == BusinessActorKind::Agent,
                    "business_agent_reviewer_assignment_required",
                )?;
                check(
                    !self
                        .business
                        .review_tasks
                        .values()
                        .any(|task| task.acceptance_id == *acceptance_id),
                    "business_review_task_already_reserved",
                )?;
                self.business.review_tasks.insert(
                    task_id.clone(),
                    BusinessReviewTask {
                        task_id: task_id.clone(),
                        acceptance_id: acceptance_id.clone(),
                        assignment_id: assignment_id.clone(),
                        assignment_version: assignment.version,
                        execution_request_id: a.execution_request_id,
                        run_id: RunId::from_uuid(a.execution_request_id.as_uuid()),
                        session_id: SessionId::new(format!(
                            "company-review-{}",
                            a.execution_request_id
                        )),
                        evidence_digest: acceptance.evidence_digest.clone(),
                        completed: false,
                    },
                );
            }
            CompanyBusinessAction::ReconcileReview { task_id } => {
                let task = self
                    .business
                    .review_tasks
                    .get(task_id)
                    .ok_or("business_review_task_missing")?
                    .clone();
                check(!task.completed, "business_review_task_completed")?;
                let reviewer = p
                    .business
                    .reviewer
                    .as_ref()
                    .ok_or("business_review_result_unconfirmed")?;
                check(
                    reviewer.run_id == task.run_id && reviewer.session_id == task.session_id,
                    "business_review_task_identity_mismatch",
                )?;
                let acceptance = self
                    .business
                    .acceptances
                    .get(&task.acceptance_id)
                    .ok_or("business_acceptance_missing")?;
                check(
                    acceptance.evidence_digest == task.evidence_digest,
                    "business_review_task_stale",
                )?;
                self.apply_business(
                    &CompanyBusinessAction::RecordReview {
                        acceptance_id: task.acceptance_id.clone(),
                        review_id: format!("review:{}", task.task_id),
                        assignment_id: task.assignment_id.clone(),
                        reviewer_run_id: Some(task.run_id),
                        results: p.business.review_results.clone(),
                    },
                    a,
                    p,
                )?;
                self.business
                    .review_tasks
                    .get_mut(task_id)
                    .unwrap()
                    .completed = true;
            }
            CompanyBusinessAction::RecordReview {
                acceptance_id,
                review_id,
                assignment_id,
                reviewer_run_id,
                results,
            } => {
                text(review_id)?;
                let acceptance = self
                    .business
                    .acceptances
                    .get(acceptance_id)
                    .ok_or("business_acceptance_missing")?
                    .clone();
                check(
                    acceptance.status == AcceptanceStatus::Requested,
                    "business_acceptance_not_reviewable",
                )?;
                let assignment = self
                    .business
                    .assignments
                    .get(assignment_id)
                    .ok_or("business_assignment_missing")?;
                check(
                    assignment.active(&a.actor_id, "reviewer", &acceptance.project_id, a.now_ms),
                    "business_reviewer_assignment_inactive",
                )?;
                let reviewer = if assignment.actor_kind == BusinessActorKind::Human {
                    check(
                        p.business.actor_is_human && reviewer_run_id.is_none(),
                        "business_human_reviewer_required",
                    )?;
                    BusinessAuthor {
                        principal_id: a.actor_id.clone(),
                        human_owner: a.actor_id.clone(),
                        role_instance_id: assignment.assignment_id.clone(),
                        session_id: a.session_id.clone(),
                        run_id: RunId::from_uuid(a.execution_request_id.as_uuid()),
                        role_id: "reviewer".into(),
                    }
                } else {
                    let reviewer = p
                        .business
                        .reviewer
                        .clone()
                        .ok_or("business_reviewer_run_required")?;
                    check(
                        Some(reviewer.run_id) == *reviewer_run_id
                            && reviewer.role_id == "reviewer"
                            && reviewer.human_owner == a.actor_id
                            && self.business.review_tasks.values().any(|task| {
                                task.run_id == reviewer.run_id
                                    && task.acceptance_id == *acceptance_id
                                    && task.assignment_id == *assignment_id
                                    && task.assignment_version == assignment.version
                            }),
                        "business_reviewer_run_mismatch",
                    )?;
                    check(
                        *results == p.business.review_results,
                        "business_review_model_result_mismatch",
                    )?;
                    reviewer
                };
                check(
                    acceptance.authors.iter().all(|author| {
                        author.principal_id != reviewer.principal_id
                            && author.role_instance_id != reviewer.role_instance_id
                            && author.session_id != reviewer.session_id
                            && author.run_id != reviewer.run_id
                    }),
                    "business_author_cannot_review",
                )?;
                check(
                    results.keys().collect::<Vec<_>>()
                        == acceptance.criteria.keys().collect::<Vec<_>>(),
                    "business_review_criterion_coverage_invalid",
                )?;
                let allowed_evidence = acceptance
                    .bundle_ids
                    .iter()
                    .filter_map(|id| self.business.bundles.get(id))
                    .flat_map(|bundle| bundle.event_refs.iter().chain(&bundle.artifact_refs))
                    .collect::<BTreeSet<_>>();
                for conclusion in results.values() {
                    text(&conclusion.reason)?;
                    refs(&conclusion.evidence_refs)?;
                    check(
                        conclusion
                            .evidence_refs
                            .iter()
                            .all(|r| p.events.contains(r) && allowed_evidence.contains(r)),
                        "business_review_evidence_unverified",
                    )?;
                }
                let target = self.business.acceptances.get_mut(acceptance_id).unwrap();
                target.review = Some(BusinessReview {
                    review_id: review_id.clone(),
                    assignment_id: assignment_id.clone(),
                    reviewer,
                    results: results.clone(),
                    evidence_digest: acceptance.evidence_digest,
                    recorded_at: a.now_ms,
                });
                target.status = AcceptanceStatus::ReadyForDecision;
            }
            CompanyBusinessAction::DecideAcceptance {
                acceptance_id,
                decision,
                reasons,
                waiver_refs,
            } => {
                check(
                    p.business.actor_is_human,
                    "business_human_decision_required",
                )?;
                let acceptance = self
                    .business
                    .acceptances
                    .get(acceptance_id)
                    .ok_or("business_acceptance_missing")?
                    .clone();
                check(
                    acceptance.status == AcceptanceStatus::ReadyForDecision,
                    "business_acceptance_not_ready",
                )?;
                let baseline = self
                    .business
                    .baselines
                    .get(&acceptance.project_id)
                    .ok_or("business_baseline_missing")?;
                check(
                    baseline.version == acceptance.baseline_version,
                    "business_acceptance_baseline_stale",
                )?;
                let review = acceptance
                    .review
                    .as_ref()
                    .ok_or("business_independent_review_required")?;
                check(
                    review.evidence_digest == acceptance.evidence_digest
                        && acceptance
                            .authors
                            .iter()
                            .all(|author| author.principal_id != a.actor_id),
                    "business_acceptance_author_or_evidence_mismatch",
                )?;
                if a.role_id == "reviewer" {
                    check(
                        review.reviewer.human_owner == a.actor_id
                            && review.reviewer.session_id == a.session_id,
                        "business_review_decider_mismatch",
                    )?;
                }
                let status = match decision {
                    AcceptanceDecision::Accept => {
                        check(
                            acceptance
                                .criteria
                                .values()
                                .filter(|c| c.required)
                                .all(|c| {
                                    review.results[&c.criterion_id].verdict
                                        == CriterionVerdict::Pass
                                }),
                            "business_required_criterion_not_passed",
                        )?;
                        AcceptanceStatus::Accepted
                    }
                    AcceptanceDecision::Reject => {
                        refs(reasons)?;
                        AcceptanceStatus::Rejected
                    }
                    AcceptanceDecision::Waive => {
                        check(
                            a.role_id == "sponsor",
                            "business_waiver_human_sponsor_required",
                        )?;
                        refs(reasons)?;
                        refs(waiver_refs)?;
                        check(
                            waiver_refs.iter().all(|r| p.events.contains(r)),
                            "business_waiver_evidence_unverified",
                        )?;
                        AcceptanceStatus::Waived
                    }
                };
                let record = self.business.acceptances.get_mut(acceptance_id).unwrap();
                record.status = status;
                record.decision_maker = Some(a.actor_id.clone());
                record.reasons = reasons.clone();
                record.waiver_refs = waiver_refs.clone();
                record.decided_at = Some(a.now_ms);
                match &acceptance.target {
                    AcceptanceTarget::Packet(_) => {}
                    AcceptanceTarget::Milestone(id) => {
                        let milestone = self.milestones.get_mut(id).unwrap();
                        milestone.status = if status == AcceptanceStatus::Accepted {
                            MilestoneStatus::Accepted
                        } else {
                            MilestoneStatus::Rejected
                        };
                        milestone.version += 1;
                    }
                    AcceptanceTarget::Project(id) => {
                        let project = self.projects.get_mut(id).unwrap();
                        project.status = if status == AcceptanceStatus::Accepted {
                            ProjectStatus::Accepted
                        } else {
                            ProjectStatus::Active
                        };
                        project.version += 1;
                    }
                }
            }
            CompanyBusinessAction::Rework {
                packet_id,
                replacement,
                reason,
                max_attempts,
            } => {
                text(reason)?;
                check(
                    (1..=3).contains(max_attempts),
                    "business_rework_limit_invalid",
                )?;
                let original = self
                    .packets
                    .get(packet_id)
                    .ok_or("business_packet_missing")?
                    .clone();
                let baseline = self
                    .business
                    .baselines
                    .get(&original.project_id)
                    .ok_or("business_baseline_missing")?
                    .clone();
                check(
                    baseline.active_packets.contains(packet_id)
                        && !self.packets.contains_key(&replacement.id)
                        && replacement.id != *packet_id,
                    "business_rework_identity_invalid",
                )?;
                check(
                    self.business.acceptances.values().any(|a| {
                        a.target == AcceptanceTarget::Packet(packet_id.clone())
                            && a.status == AcceptanceStatus::Rejected
                            && a.baseline_version == baseline.version
                    }),
                    "business_rework_rejection_required",
                )?;
                check(
                    self.runs
                        .get(packet_id)
                        .is_some_and(|r| r.status == ExecutionStatus::Completed),
                    "business_rework_unresolved_attempt",
                )?;
                let mut predecessor = packet_id.clone();
                let mut count = 0;
                while let Some((previous, _)) = baseline
                    .superseded_packets
                    .iter()
                    .find(|(_, next)| *next == &predecessor)
                {
                    predecessor = previous.clone();
                    count += 1;
                    check(count < *max_attempts, "business_rework_limit_exhausted")?;
                }
                check(
                    replacement.path_allow == original.packet.path_allow
                        && replacement.acceptance == original.packet.acceptance
                        && replacement.acceptance_tests == original.packet.acceptance_tests
                        && replacement.dependencies == original.packet.dependencies,
                    "business_rework_cannot_change_baseline",
                )?;
                let was_rejected = self
                    .milestones
                    .get(&original.milestone_id)
                    .is_some_and(|m| m.status == MilestoneStatus::Rejected);
                if was_rejected {
                    self.milestones
                        .get_mut(&original.milestone_id)
                        .unwrap()
                        .status = MilestoneStatus::Active;
                }
                check(
                    !self.runs.values().any(|run| {
                        run.project_id == original.project_id && !run.status.is_terminal()
                    }),
                    "business_rework_active_run_denied",
                )?;
                self.apply(
                    &CompanyCommand::ApprovePacket {
                        project_id: original.project_id.clone(),
                        milestone_id: original.milestone_id.clone(),
                        packet: replacement.clone(),
                    },
                    a,
                    p,
                )?;
                let baseline = self
                    .business
                    .baselines
                    .get_mut(&original.project_id)
                    .unwrap();
                baseline.active_packets.retain(|id| id != packet_id);
                baseline.active_packets.push(replacement.id.clone());
                baseline
                    .superseded_packets
                    .insert(packet_id.clone(), replacement.id.clone());
                for criterion in baseline.criteria.values_mut().filter(|criterion| {
                    criterion.target == AcceptanceTarget::Packet(packet_id.clone())
                }) {
                    criterion.target = AcceptanceTarget::Packet(replacement.id.clone());
                }
                for packet in self
                    .packets
                    .values_mut()
                    .filter(|p| p.project_id == original.project_id)
                {
                    for dep in &mut packet.packet.dependencies {
                        if dep == packet_id {
                            *dep = replacement.id.clone();
                        }
                    }
                }
                if let Some(m) = self.milestones.get_mut(&original.milestone_id) {
                    if was_rejected {
                        m.status = MilestoneStatus::Rework;
                        m.version += 1;
                    }
                }
            }
            CompanyBusinessAction::Pause { project_id } => {
                let project = self
                    .projects
                    .get_mut(project_id)
                    .ok_or("business_project_missing")?;
                check(
                    matches!(
                        project.status,
                        ProjectStatus::Approved
                            | ProjectStatus::Planned
                            | ProjectStatus::Active
                            | ProjectStatus::AtRisk
                            | ProjectStatus::ChangePending
                            | ProjectStatus::ReadyForAcceptance
                    ),
                    "business_project_not_pausable",
                )?;
                self.business
                    .paused_from
                    .insert(project_id.clone(), project.status);
                project.status = ProjectStatus::Paused;
                project.version += 1;
            }
            CompanyBusinessAction::Resume { project_id } => {
                let previous = self
                    .business
                    .paused_from
                    .remove(project_id)
                    .ok_or("business_pause_origin_missing")?;
                let project = self
                    .projects
                    .get_mut(project_id)
                    .ok_or("business_project_missing")?;
                check(
                    project.status == ProjectStatus::Paused,
                    "business_project_not_paused",
                )?;
                project.status = previous;
                project.version += 1;
            }
        }
        self.business.clock_ms = a.now_ms;
        Ok(())
    }
}
fn validate_criteria(
    criteria: &[BusinessCriterion],
    target: &AcceptanceTarget,
) -> Result<BTreeMap<String, BusinessCriterion>> {
    check(
        !criteria.is_empty() && criteria.len() <= 1024,
        "business_criteria_required",
    )?;
    let mut result = BTreeMap::new();
    for criterion in criteria {
        text(&criterion.criterion_id)?;
        text(&criterion.description)?;
        check(
            criterion.target == *target
                && criterion.refines.is_empty()
                && !result.contains_key(&criterion.criterion_id),
            "business_charter_criterion_invalid",
        )?;
        result.insert(criterion.criterion_id.clone(), criterion.clone());
    }
    check(
        result.values().any(|c| c.required),
        "business_required_criterion_missing",
    )?;
    Ok(result)
}
