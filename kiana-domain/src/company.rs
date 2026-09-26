//! Versioned Company business contracts. State is rebuilt exclusively from committed events.
//! This module is deterministic: clocks, identity and observed runtime evidence are inputs.
use crate::{
    journal_sha256, ExecutionStatus, RequestId, RunId, RuntimeReceiptRef, SessionId, WorkPacket,
    WorkPacketStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const COMPANY_COMMAND_SCHEMA: &str = "kiana.company-command.v1";
pub const COMPANY_EVENT_SCHEMA: &str = "kiana.company-event.v1";
pub const COMPANY_STATE_SCHEMA: &str = "kiana.company-state.v1";
pub const COMPANY_COMMAND: &str = "company.command.v1";
pub const COMPANY_SNAPSHOT: &str = "company.snapshot.v1";
pub const COMPANY_GOVERNANCE: &str = "company.governance.v1";

pub type CompanyResult<T> = Result<T, &'static str>;
fn one() -> u64 {
    1
}
fn required(value: &str) -> CompanyResult<()> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("company_field_required_or_too_large")
    } else {
        Ok(())
    }
}

fn charter_identity_digest(artifact: &CompanyArtifact) -> String {
    crate::json_digest(&serde_json::json!({
        "content_hash": journal_sha256(artifact.text.as_bytes()),
        "typed_version": artifact.typed_version.as_ref(),
    }))
}
fn list(values: &[String]) -> CompanyResult<()> {
    if values.is_empty() || values.len() > 1024 {
        return Err("company_evidence_or_criteria_required");
    }
    for value in values {
        required(value)?;
    }
    let mut distinct = values.to_vec();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != values.len() {
        return Err("company_duplicate_reference");
    }
    Ok(())
}

fn runtime_receipt_matches(run: &CompanyRun, evidence_refs: &[String]) -> bool {
    run.runtime_receipt.as_ref().is_some_and(|receipt| {
        receipt.validate().is_ok()
            && receipt.request_id == run.execution_request_id
            && receipt.status == run.status
            && receipt
                .event_refs
                .iter()
                .all(|reference| evidence_refs.contains(reference))
    })
}
fn ensure(condition: bool, reason: &'static str) -> CompanyResult<()> {
    if condition {
        Ok(())
    } else {
        Err(reason)
    }
}

macro_rules! states {
    ($name:ident, $first:ident $(, $rest:ident)*; $($from:ident => $to:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all="snake_case")]
        pub enum $name { #[default] $first, $($rest),* }
        impl $name {
            pub fn transition(self, next: Self) -> CompanyResult<Self> {
                if matches!((self,next), $((Self::$from,Self::$to))|*) { Ok(next) }
                else { Err("company_illegal_state_transition") }
            }
        }
    }
}
states!(ObjectiveStatus, Proposed, Active, AtRisk, Paused, Rejected, Achieved, Abandoned, Archived;
 Proposed=>Active, Proposed=>Rejected, Active=>AtRisk, AtRisk=>Active, Active=>Paused, AtRisk=>Paused,
 Paused=>Active, Active=>Achieved, Active=>Abandoned, AtRisk=>Abandoned, Paused=>Abandoned,
 Rejected=>Archived, Achieved=>Archived, Abandoned=>Archived);
states!(InitiativeStatus, Intake, Triaged, Assessed, Approved, Rejected, ConvertedToProject, Closed, Archived;
 Intake=>Triaged, Triaged=>Assessed, Assessed=>Approved, Assessed=>Rejected, Approved=>ConvertedToProject,
 ConvertedToProject=>Closed, Rejected=>Archived);
states!(ProjectStatus, Proposed, Chartering, Approved, Planned, Active, Paused, AtRisk, ChangePending,
 CancelRequested, Cancelled, ResultUnknown, ReadyForAcceptance, Accepted, Failed, Closed, Rejected, Archived;
 Proposed=>Chartering, Chartering=>Approved, Chartering=>Rejected, Approved=>Planned, Planned=>Active,
 Active=>Paused, AtRisk=>Paused, ChangePending=>Paused, Paused=>Active, Active=>AtRisk, AtRisk=>Active,
 Active=>ChangePending, ChangePending=>Active, Active=>CancelRequested, Paused=>CancelRequested,
 AtRisk=>CancelRequested, ChangePending=>CancelRequested, CancelRequested=>Active,
 CancelRequested=>Cancelled, CancelRequested=>ResultUnknown, Active=>ReadyForAcceptance,
 ReadyForAcceptance=>Accepted, ReadyForAcceptance=>Active, ReadyForAcceptance=>Closed,
 Accepted=>Closed, Active=>Failed, Failed=>Archived, Cancelled=>Archived, Closed=>Archived, Rejected=>Archived);
states!(MilestoneStatus, Planned, Active, Blocked, ReadyForAcceptance, Accepted, Rejected, Rework, Cancelled, Closed;
 Planned=>Active, Active=>Blocked, Blocked=>Active, Active=>ReadyForAcceptance, Active=>Cancelled,
 Blocked=>Cancelled, ReadyForAcceptance=>Accepted, ReadyForAcceptance=>Rejected,
 Rejected=>Rework, Rework=>ReadyForAcceptance, Accepted=>Closed);
states!(AcceptanceStatus, Requested, EvidencePending, ReadyForDecision, Accepted, Rejected, Waived, ReworkRequested, Closed;
 Requested=>EvidencePending, EvidencePending=>ReadyForDecision, ReadyForDecision=>Accepted,
 ReadyForDecision=>Rejected, ReadyForDecision=>Waived, Rejected=>ReworkRequested,
 ReworkRequested=>EvidencePending, Rejected=>Closed, Accepted=>Closed, Waived=>Closed);
states!(DeliveryStatus, Prepared, Approved, Delivered, Confirmed, DeliveryUnknown, Reconciled, Failed;
 Prepared=>Approved, Approved=>Delivered, Delivered=>Confirmed, Delivered=>DeliveryUnknown,
 DeliveryUnknown=>Reconciled, Reconciled=>Confirmed, Reconciled=>Failed);
states!(OutcomeStatus, Planned, Measuring, Realized, PartiallyRealized, NotRealized;
 Planned=>Measuring, Measuring=>Realized, Measuring=>PartiallyRealized, Measuring=>NotRealized);
states!(ChangeStatus, Draft, ImpactAssessed, PendingDecision, Approved, Rejected, Implementing, Verified,
 Blocked, CancelRequested, Cancelled, Failed, Closed, Archived;
 Draft=>ImpactAssessed, ImpactAssessed=>PendingDecision, PendingDecision=>Approved,
 PendingDecision=>Rejected, Approved=>Implementing, Implementing=>Verified, Verified=>Closed,
 Implementing=>Blocked, Blocked=>Implementing, Implementing=>CancelRequested, Blocked=>CancelRequested,
 CancelRequested=>Cancelled, CancelRequested=>Implementing, Implementing=>Failed, Blocked=>Failed,
 Rejected=>Archived, Failed=>Archived, Cancelled=>Archived, Closed=>Archived);
states!(RiskStatus, Identified, Assessed, Mitigating, Monitoring, Materialized, Closed;
 Identified=>Assessed, Assessed=>Mitigating, Mitigating=>Monitoring, Monitoring=>Closed,
 Mitigating=>Materialized, Monitoring=>Materialized, Materialized=>Closed);
states!(IncidentStatus, Open, Triaged, Assigned, Mitigating, Monitoring, Resolved, Escalated, Closed;
 Open=>Triaged, Triaged=>Assigned, Assigned=>Mitigating, Mitigating=>Monitoring,
 Monitoring=>Resolved, Resolved=>Closed, Triaged=>Escalated, Escalated=>Mitigating);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricDirection {
    AtLeast,
    AtMost,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Objective {
    pub objective_id: String,
    pub organization_id: String,
    pub title: String,
    pub problem: String,
    pub metric: String,
    pub baseline: f64,
    pub target: f64,
    pub unit: String,
    /// How the metric is observed. Legacy proposals may omit it, but activation requires it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement_method: Option<String>,
    pub direction: MetricDirection,
    pub period_start: u64,
    pub period_end: u64,
    pub owner_principal_id: String,
    #[serde(default)]
    pub status: ObjectiveStatus,
    #[serde(default = "one")]
    pub version: u64,
}
impl Objective {
    pub fn validate(&self) -> CompanyResult<()> {
        for v in [
            &self.objective_id,
            &self.organization_id,
            &self.title,
            &self.problem,
            &self.metric,
            &self.unit,
            &self.owner_principal_id,
        ] {
            required(v)?;
        }
        ensure(
            self.baseline.is_finite()
                && self.target.is_finite()
                && match self.direction {
                    MetricDirection::AtLeast => self.target > self.baseline,
                    MetricDirection::AtMost => self.target < self.baseline,
                }
                && self
                    .measurement_method
                    .as_deref()
                    .is_none_or(|method| !method.trim().is_empty() && method.len() <= 16_384)
                && self.period_end > self.period_start
                && self.period_start > 0
                && self.version > 0,
            "objective_measurement_invalid",
        )
    }
    pub fn target_met(&self, value: f64) -> bool {
        match self.direction {
            MetricDirection::AtLeast => value >= self.target,
            MetricDirection::AtMost => value <= self.target,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Initiative {
    pub initiative_id: String,
    pub organization_id: String,
    pub objective_refs: Vec<String>,
    pub title: String,
    pub problem_statement: String,
    pub hypothesis: String,
    pub sponsor_id: String,
    pub expected_value: String,
    pub rough_cost: String,
    pub risk_summary: String,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub status: InitiativeStatus,
    #[serde(default = "one")]
    pub version: u64,
}
impl Initiative {
    pub fn validate(&self) -> CompanyResult<()> {
        for v in [
            &self.initiative_id,
            &self.organization_id,
            &self.title,
            &self.problem_statement,
            &self.hypothesis,
            &self.sponsor_id,
            &self.expected_value,
            &self.rough_cost,
            &self.risk_summary,
        ] {
            required(v)?;
        }
        list(&self.objective_refs)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub project_id: String,
    pub organization_id: String,
    pub objective_refs: Vec<String>,
    pub sponsor_id: String,
    pub charter_ref: String,
    pub scope_baseline: String,
    pub success_criteria: Vec<String>,
    pub non_goals: Vec<String>,
    pub project_budget_ref: String,
    pub risk_summary: String,
    #[serde(default)]
    pub decision_ref: Option<String>,
    #[serde(default)]
    pub milestone_refs: Vec<String>,
    #[serde(default)]
    pub incident_id: Option<String>,
    #[serde(default)]
    pub acceptance_id: Option<String>,
    #[serde(default)]
    pub closing_receipt_id: Option<String>,
    #[serde(default)]
    pub status: ProjectStatus,
    #[serde(default = "one")]
    pub version: u64,
}
impl Project {
    pub fn validate(&self) -> CompanyResult<()> {
        for v in [
            &self.project_id,
            &self.organization_id,
            &self.sponsor_id,
            &self.charter_ref,
            &self.scope_baseline,
            &self.project_budget_ref,
            &self.risk_summary,
        ] {
            required(v)?;
        }
        list(&self.objective_refs)?;
        list(&self.success_criteria)?;
        list(&self.non_goals)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCharterBaseline {
    pub project_id: String,
    pub version: u64,
    pub charter_ref: String,
    pub scope_baseline: String,
    pub success_criteria: Vec<String>,
    pub non_goals: Vec<String>,
    pub risk_summary: String,
    pub project_budget_ref: String,
    pub budget_digest: String,
}

impl ProjectCharterBaseline {
    fn from_project(project: &Project, budget: &CompanyBudgetPolicy) -> Self {
        Self {
            project_id: project.project_id.clone(),
            version: 1,
            charter_ref: project.charter_ref.clone(),
            scope_baseline: project.scope_baseline.clone(),
            success_criteria: project.success_criteria.clone(),
            non_goals: project.non_goals.clone(),
            risk_summary: project.risk_summary.clone(),
            project_budget_ref: project.project_budget_ref.clone(),
            budget_digest: crate::json_digest(&serde_json::json!(budget)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Milestone {
    pub milestone_id: String,
    pub project_id: String,
    pub objective_refs: Vec<String>,
    pub deliverables: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub due_at: u64,
    #[serde(default)]
    pub dependency_refs: Vec<String>,
    #[serde(default)]
    pub status: MilestoneStatus,
    #[serde(default = "one")]
    pub version: u64,
}
impl Milestone {
    pub fn validate(&self) -> CompanyResult<()> {
        required(&self.milestone_id)?;
        required(&self.project_id)?;
        list(&self.objective_refs)?;
        list(&self.deliverables)?;
        list(&self.acceptance_criteria)?;
        ensure(self.due_at > 0, "milestone_due_at_required")
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriteriaSnapshot {
    pub project_version: u64,
    pub milestone_version: u64,
    pub packet_version: u64,
    pub milestone_versions: BTreeMap<String, u64>,
    pub packet_versions: BTreeMap<String, u64>,
    pub project_criteria: Vec<String>,
    pub milestone_criteria: Vec<String>,
    pub packet_criteria: Vec<String>,
    /// Versioned criterion objects; legacy text lists remain for replay compatibility.
    #[serde(default)]
    pub criterion_refs: Vec<crate::Criterion>,
}
impl CriteriaSnapshot {
    pub fn validate(&self) -> CompanyResult<()> {
        ensure(
            self.project_version > 0 && self.milestone_version > 0 && self.packet_version > 0,
            "criteria_snapshot_version_required",
        )?;
        list(&self.project_criteria)?;
        list(&self.milestone_criteria)?;
        list(&self.packet_criteria)?;
        for criterion in &self.criterion_refs {
            criterion
                .validate()
                .map_err(|_| "criteria_snapshot_criterion_invalid")?;
        }
        Ok(())
    }

    pub fn criteria(&self) -> Vec<String> {
        if !self.criterion_refs.is_empty() {
            let mut ids = self
                .criterion_refs
                .iter()
                .map(|criterion| criterion.criterion_id.to_string())
                .collect::<Vec<_>>();
            ids.sort();
            ids.dedup();
            return ids;
        }
        let mut all = self
            .project_criteria
            .iter()
            .chain(&self.milestone_criteria)
            .chain(&self.packet_criteria)
            .cloned()
            .collect::<Vec<_>>();
        all.sort();
        all.dedup();
        all
    }

    pub fn typed_criteria(&self) -> &[crate::Criterion] {
        &self.criterion_refs
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    pub acceptance_id: String,
    pub project_id: String,
    pub milestone_id: String,
    pub work_packet_id: String,
    pub criteria_snapshot: CriteriaSnapshot,
    pub evidence_refs: Vec<String>,
    pub author_run_id: RunId,
    pub author_session_id: SessionId,
    pub reviewer_id: Option<String>,
    pub decision_maker_id: Option<String>,
    pub decision: Option<AcceptanceDecision>,
    pub decided_at: Option<u64>,
    pub rejection_reasons: Vec<String>,
    pub status: AcceptanceStatus,
    pub version: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceDecision {
    Accept,
    Reject,
    Waive,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReview {
    pub review_id: String,
    pub acceptance_id: String,
    pub author_run_id: RunId,
    pub reviewer_session_id: SessionId,
    pub reviewer_id: String,
    pub criteria_snapshot: CriteriaSnapshot,
    pub criterion_results: BTreeMap<String, bool>,
    pub evidence_refs: Vec<String>,
    pub recorded_at: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub delivery_id: String,
    pub project_id: String,
    pub acceptance_id: String,
    pub artifact_refs: Vec<String>,
    pub recipient_ref: String,
    pub handoff_receipt_ref: Option<String>,
    pub delivered_at: Option<u64>,
    #[serde(default)]
    pub incident_id: Option<String>,
    #[serde(default)]
    pub status: DeliveryStatus,
    #[serde(default = "one")]
    pub version: u64,
}
impl Delivery {
    pub fn validate(&self) -> CompanyResult<()> {
        required(&self.delivery_id)?;
        required(&self.project_id)?;
        required(&self.acceptance_id)?;
        required(&self.recipient_ref)?;
        list(&self.artifact_refs)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricObservation {
    pub value: f64,
    pub observed_at: u64,
    pub evidence_refs: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub outcome_id: String,
    pub objective_id: String,
    pub project_id: String,
    pub metric_observations: Vec<MetricObservation>,
    pub target_snapshot: Objective,
    pub measurement_start: u64,
    pub measurement_end: u64,
    pub owner_id: String,
    pub review_at: u64,
    pub evidence_refs: Vec<String>,
    pub status: OutcomeStatus,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRequest {
    pub change_id: String,
    pub project_id: String,
    pub requested_by: String,
    pub reason: String,
    pub affected_scope: Vec<String>,
    pub affected_objectives: Vec<String>,
    pub affected_budget: String,
    pub affected_schedule: String,
    pub affected_risk: String,
    pub proposed_baseline_version: u64,
    pub proposed_scope_baseline: String,
    pub proposed_success_criteria: Vec<String>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub status: ChangeStatus,
    #[serde(default = "one")]
    pub version: u64,
}
impl ChangeRequest {
    pub fn validate(&self) -> CompanyResult<()> {
        for v in [
            &self.change_id,
            &self.project_id,
            &self.requested_by,
            &self.reason,
            &self.affected_budget,
            &self.affected_schedule,
            &self.affected_risk,
            &self.proposed_scope_baseline,
        ] {
            required(v)?;
        }
        list(&self.affected_scope)?;
        list(&self.proposed_success_criteria)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Risk {
    pub risk_id: String,
    pub project_id: String,
    pub description: String,
    pub probability: f64,
    pub impact: String,
    pub trigger: String,
    pub mitigation: String,
    pub contingency: String,
    pub owner_id: String,
    #[serde(default)]
    pub incident_id: Option<String>,
    #[serde(default)]
    pub status: RiskStatus,
}
impl Risk {
    pub fn validate(&self) -> CompanyResult<()> {
        for v in [
            &self.risk_id,
            &self.project_id,
            &self.description,
            &self.impact,
            &self.trigger,
            &self.mitigation,
            &self.contingency,
            &self.owner_id,
        ] {
            required(v)?;
        }
        ensure(
            self.probability.is_finite() && (0.0..=1.0).contains(&self.probability),
            "risk_probability_invalid",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Incident {
    pub incident_id: String,
    pub project_id: Option<String>,
    pub run_id: Option<RunId>,
    pub risk_id: Option<String>,
    pub delivery_id: Option<String>,
    pub severity: String,
    pub detected_at: u64,
    pub impact: String,
    pub timeline: Vec<String>,
    pub owner_id: String,
    pub response_actions: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub escalation_target: Option<String>,
    #[serde(default)]
    pub status: IncidentStatus,
}
impl Incident {
    pub fn validate(&self) -> CompanyResult<()> {
        for v in [
            &self.incident_id,
            &self.severity,
            &self.impact,
            &self.owner_id,
        ] {
            required(v)?;
        }
        ensure(self.detected_at > 0, "incident_detected_at_required")?;
        list(&self.evidence_refs)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompanyArtifact {
    pub artifact_id: String,
    pub relative_path: String,
    /// The exact, bounded, redacted-at-admission content snapshot. References never silently
    /// follow the mutable workspace file to a later revision.
    pub text: String,
    pub registered_at: u64,
    /// Optional typed immutable version; legacy text snapshots remain readable during migration.
    #[serde(default)]
    pub typed_version: Option<crate::ArtifactVersion>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompanyPacket {
    pub project_id: String,
    pub milestone_id: String,
    pub packet: WorkPacket,
    pub version: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompanyRun {
    pub packet_id: String,
    pub project_id: String,
    pub execution_request_id: RequestId,
    pub author_session_id: SessionId,
    pub run_id: Option<RunId>,
    pub status: ExecutionStatus,
    pub evidence_refs: Vec<String>,
    /// ControlPlane-derived runtime terminal; business acceptance remains separate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_receipt: Option<RuntimeReceiptRef>,
    pub incident_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompanyBudgetPolicy {
    pub project: crate::ProjectBudget,
    pub runtime: crate::RuntimeBudget,
    pub quota: crate::Quota,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompanyClosingReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub project_id: String,
    pub acceptance_id: String,
    pub delivery_id: String,
    pub review_id: String,
    pub author_run_id: RunId,
    pub author_session_id: SessionId,
    pub reviewer_session_id: SessionId,
    pub closer_session_id: SessionId,
    pub artifact_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub closed_at: u64,
    pub waiver_ref: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyCommandRequest {
    pub schema: String,
    pub expected_revision: u64,
    pub idempotency_key: String,
    pub command: CompanyCommand,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompanyCommand {
    Business {
        schema: String,
        action: Box<crate::CompanyBusinessAction>,
    },
    RegisterArtifact {
        artifact_id: String,
        relative_path: String,
    },
    ProposeObjective {
        objective: Objective,
    },
    DecideObjective {
        objective_id: String,
        approve: bool,
    },
    AchieveObjective {
        objective_id: String,
    },
    SubmitInitiative {
        initiative: Initiative,
    },
    AdvanceInitiative {
        initiative_id: String,
        status: InitiativeStatus,
        decision: Option<String>,
        project_id: Option<String>,
    },
    ProposeProject {
        project: Project,
    },
    StartChartering {
        project_id: String,
    },
    ApproveProject {
        project_id: String,
        decision_ref: String,
    },
    RejectProject {
        project_id: String,
        decision_ref: String,
    },
    CreateMilestone {
        milestone: Milestone,
    },
    ApprovePacket {
        project_id: String,
        milestone_id: String,
        packet: WorkPacket,
    },
    ReworkPacket {
        packet_id: String,
        replacement: WorkPacket,
    },
    PlanProject {
        project_id: String,
    },
    AcknowledgeHandoff {
        handoff_id: String,
        accept: bool,
        reason: String,
    },
    ReviewPacket {
        packet_id: String,
        review_id: String,
        criterion_results: BTreeMap<String, bool>,
        evidence_refs: Vec<String>,
    },
    ConfigureBudget {
        project_id: String,
        policy: CompanyBudgetPolicy,
    },
    ClaimPacket {
        packet_id: String,
    },
    RenewPacketClaim {
        packet_id: String,
    },
    ReclaimPacketClaim {
        packet_id: String,
    },
    StartRun {
        project_id: String,
        packet_id: String,
        sandbox: Option<String>,
    },
    ReconcileRun {
        packet_id: String,
    },
    RecordRunStarted {
        packet_id: String,
    },
    RequestAcceptance {
        acceptance_id: String,
        project_id: String,
        packet_id: String,
        evidence_refs: Vec<String>,
    },
    RecordReview {
        review_id: String,
        acceptance_id: String,
        criterion_results: BTreeMap<String, bool>,
        evidence_refs: Vec<String>,
    },
    DecideAcceptance {
        acceptance_id: String,
        review_id: String,
        decision: AcceptanceDecision,
        reasons: Vec<String>,
        waiver_ref: Option<String>,
    },
    PrepareDelivery {
        delivery: Delivery,
    },
    ApproveDelivery {
        delivery_id: String,
    },
    Deliver {
        delivery_id: String,
        evidence_refs: Vec<String>,
    },
    ConfirmDelivery {
        delivery_id: String,
        handoff_receipt_ref: String,
    },
    MarkDeliveryUnknown {
        delivery_id: String,
        incident: Incident,
    },
    ReconcileDelivery {
        delivery_id: String,
        confirmed: bool,
        evidence_refs: Vec<String>,
    },
    CloseProject {
        project_id: String,
        delivery_id: String,
        receipt_id: String,
        evidence_refs: Vec<String>,
    },
    RecordOutcome {
        outcome_id: String,
        objective_id: String,
        project_id: String,
        observation: MetricObservation,
    },
    RequestChange {
        change: ChangeRequest,
    },
    DecideChange {
        change_id: String,
        approve: bool,
        decision_ref: String,
    },
    IdentifyRisk {
        risk: Risk,
    },
    AdvanceRisk {
        risk_id: String,
        status: RiskStatus,
        incident_id: Option<String>,
    },
    OpenIncident {
        incident: Incident,
    },
    AdvanceIncident {
        incident_id: String,
        status: IncidentStatus,
        evidence_refs: Vec<String>,
    },
    PauseProject {
        project_id: String,
    },
    ResumeProject {
        project_id: String,
    },
    RequestCancelProject {
        project_id: String,
        reason: String,
    },
    ConfirmCancelProject {
        project_id: String,
        incident: Option<Incident>,
    },
    FailProject {
        project_id: String,
        reason: String,
        evidence_refs: Vec<String>,
    },
    ArchiveProject {
        project_id: String,
    },
}
impl CompanyCommand {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Business { .. } => "business.v2",
            Self::RegisterArtifact { .. } => "ArtifactRegistered",
            Self::ProposeObjective { .. } => "ObjectiveProposed",
            Self::DecideObjective { approve: true, .. } => "ObjectiveActivated",
            Self::DecideObjective { .. } => "ObjectiveRejected",
            Self::AchieveObjective { .. } => "ObjectiveAchieved",
            Self::SubmitInitiative { .. } => "InitiativeSubmitted",
            Self::AdvanceInitiative { .. } => "InitiativeTransitioned",
            Self::ProposeProject { .. } => "ProjectProposed",
            Self::StartChartering { .. } => "ProjectCharteringStarted",
            Self::ApproveProject { .. } => "ProjectApproved",
            Self::RejectProject { .. } => "ProjectRejected",
            Self::CreateMilestone { .. } => "MilestoneCreated",
            Self::ApprovePacket { .. } => "PacketApproved",
            Self::ReworkPacket { .. } => "PacketReworkApproved",
            Self::PlanProject { .. } => "ProjectPlanned",
            Self::AcknowledgeHandoff { .. } => "HandoffAcknowledged",
            Self::ReviewPacket { .. } => "PacketReviewed",
            Self::ConfigureBudget { .. } => "ProjectBudgetConfigured",
            Self::ClaimPacket { .. } => "PacketClaimed",
            Self::RenewPacketClaim { .. } => "PacketClaimRenewed",
            Self::ReclaimPacketClaim { .. } => "PacketClaimReclaimed",
            Self::StartRun { .. } => "RunStartRequested",
            Self::ReconcileRun { .. } => "RunObserved",
            Self::RecordRunStarted { .. } => "RunStarted",
            Self::RequestAcceptance { .. } => "AcceptanceRequested",
            Self::RecordReview { .. } => "ReviewRecorded",
            Self::DecideAcceptance { .. } => "AcceptanceDecided",
            Self::PrepareDelivery { .. } => "DeliveryPrepared",
            Self::ApproveDelivery { .. } => "DeliveryApproved",
            Self::Deliver { .. } => "Delivered",
            Self::ConfirmDelivery { .. } => "DeliveryConfirmed",
            Self::MarkDeliveryUnknown { .. } => "DeliveryUnknown",
            Self::ReconcileDelivery { .. } => "DeliveryReconciled",
            Self::CloseProject { .. } => "ProjectClosed",
            Self::RecordOutcome { .. } => "OutcomeRecorded",
            Self::RequestChange { .. } => "ChangeRequested",
            Self::DecideChange { approve: true, .. } => "ChangeApproved",
            Self::DecideChange { .. } => "ChangeRejected",
            Self::IdentifyRisk { .. } => "RiskIdentified",
            Self::AdvanceRisk { .. } => "RiskTransitioned",
            Self::OpenIncident { .. } => "IncidentOpened",
            Self::AdvanceIncident { .. } => "IncidentTransitioned",
            Self::PauseProject { .. } => "ProjectPaused",
            Self::ResumeProject { .. } => "ProjectResumed",
            Self::RequestCancelProject { .. } => "ProjectCancelRequested",
            Self::ConfirmCancelProject {
                incident: Some(_), ..
            } => "ProjectResultUnknown",
            Self::ConfirmCancelProject { .. } => "ProjectCancelled",
            Self::FailProject { .. } => "ProjectFailed",
            Self::ArchiveProject { .. } => "ProjectArchived",
        }
    }
    pub fn allowed_roles(&self) -> &'static [&'static str] {
        match self {
            Self::Business { action, .. } => action.roles(),
            Self::ConfigureBudget { .. }
            | Self::ProposeObjective { .. }
            | Self::DecideObjective { .. }
            | Self::AchieveObjective { .. }
            | Self::ApproveProject { .. }
            | Self::RejectProject { .. }
            | Self::DecideChange { .. } => &["sponsor"],
            Self::ClaimPacket { .. } | Self::RenewPacketClaim { .. } => &["builder"],
            Self::StartRun { .. } | Self::RequestAcceptance { .. } => &["builder", "closer"],
            Self::ReviewPacket { .. } | Self::RecordReview { .. } => &["reviewer"],
            Self::DecideAcceptance { .. } => &["reviewer", "sponsor"],
            Self::PrepareDelivery { .. }
            | Self::ApproveDelivery { .. }
            | Self::Deliver { .. }
            | Self::CloseProject { .. }
            | Self::RecordOutcome { .. }
            | Self::MarkDeliveryUnknown { .. }
            | Self::ReconcileDelivery { .. } => &["closer", "sponsor"],
            Self::CreateMilestone { .. }
            | Self::ApprovePacket { .. }
            | Self::ReworkPacket { .. }
            | Self::PlanProject { .. } => &["pm"],
            Self::ProposeProject { .. }
            | Self::StartChartering { .. }
            | Self::PauseProject { .. }
            | Self::ResumeProject { .. }
            | Self::RequestChange { .. } => &["sponsor", "pm"],
            Self::RequestCancelProject { .. }
            | Self::ConfirmCancelProject { .. }
            | Self::FailProject { .. }
            | Self::ArchiveProject { .. } => &["sponsor", "closer"],
            _ => &[
                "sponsor",
                "pm",
                "architect",
                "builder",
                "reviewer",
                "closer",
            ],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompanyAuthority {
    pub actor_id: String,
    pub role_id: String,
    pub session_id: SessionId,
    pub now_ms: u64,
    pub execution_request_id: RequestId,
    #[serde(default)]
    pub execution_cell_id: Option<crate::CellId>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyProof {
    #[serde(default)]
    pub business: crate::BusinessProof,
    pub artifact: Option<CompanyArtifact>,
    /// Typed immutable artifact metadata; legacy `artifact` remains for compatibility.
    #[serde(default)]
    pub artifact_version: Option<crate::ArtifactVersion>,
    /// Typed immutable evidence links; legacy `events` strings remain replay-compatible.
    #[serde(default)]
    pub typed_evidence_refs: Vec<crate::EvidenceRef>,
    /// Explicit post-commit effect handoff; its status never implies effect success.
    #[serde(default)]
    pub dispatch_intent: Option<crate::DispatchIntent>,
    /// Typed evidence references derived by ControlPlane, never trusted from command JSON.
    #[serde(default)]
    pub evidence_refs: Vec<crate::EvidenceRef>,
    /// Always derived by ControlPlane from persisted events; never accepted from a command.
    pub run: Option<CompanyRun>,
    pub events: Vec<String>,
    pub project_runs_stopped: bool,
    #[serde(default)]
    pub closeout: crate::CompanyCloseoutProof,
    /// Server-generated business decision record; never accepted from a model/UI payload.
    #[serde(default)]
    pub human_decision: Option<crate::HumanDecision>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyState {
    #[serde(default)]
    pub business: crate::CompanyBusinessState,
    #[serde(default)]
    pub budgets: BTreeMap<String, CompanyBudgetPolicy>,
    #[serde(default)]
    pub handoffs: BTreeMap<String, crate::PacketHandoff>,
    /// CO-17 accountability handoffs are kept as a typed projection alongside the legacy
    /// planning-to-executing handoff map.  Both maps remain replay/serde compatible.
    #[serde(default)]
    pub accountability_handoffs: crate::CompanyHandoffLedger,
    /// CO-19 append-only attempt/epoch history; legacy PacketClaim remains readable.
    #[serde(default)]
    pub packet_attempts: crate::PacketAttemptLedger,
    /// CO-31 append-only project pause/resume/cancel propagation plans. Runtime effects remain
    /// fenced by the existing cancellation, dispatch and cell registries.
    #[serde(default)]
    pub project_controls: crate::ProjectControlLedger,
    /// CO-32 separates risk signals, named incidents and append-only Unknown reconciliation.
    #[serde(default)]
    pub company_reconciliation: crate::CompanyReconciliationLedger,
    /// CO-33 immutable accepted-artifact manifests and local package descriptors.
    #[serde(default)]
    pub delivery_manifests: crate::DeliveryManifestLedger,
    /// CO-34 authorization, one-shot dispatch, Broker effect and recipient confirmation facts.
    #[serde(default)]
    pub delivery_authorizations: crate::DeliveryAuthorizationLedger,
    /// CO-35 honest success/failure/cancel/waiver closeout contracts.
    #[serde(default)]
    pub closing_receipt_contracts: crate::CompanyClosingReceiptLedger,
    #[serde(default)]
    pub packet_reviews: BTreeMap<String, crate::PacketReview>,
    pub artifacts: BTreeMap<String, CompanyArtifact>,
    /// Digest captured when a project proposal references a registered Charter.
    /// Legacy/replayed projects may omit it and still require a present artifact at approval.
    #[serde(default)]
    pub charter_digests: BTreeMap<String, String>,
    /// Frozen scope/criteria/budget references created by a successful Charter approval. Project
    /// state revisions continue to advance independently after this snapshot.
    #[serde(default)]
    pub charter_baselines: BTreeMap<String, ProjectCharterBaseline>,
    pub revision: u64,
    pub objectives: BTreeMap<String, Objective>,
    pub initiatives: BTreeMap<String, Initiative>,
    pub projects: BTreeMap<String, Project>,
    pub milestones: BTreeMap<String, Milestone>,
    pub packets: BTreeMap<String, CompanyPacket>,
    pub runs: BTreeMap<String, CompanyRun>,
    pub acceptances: BTreeMap<String, Acceptance>,
    pub reviews: BTreeMap<String, CompanyReview>,
    pub deliveries: BTreeMap<String, Delivery>,
    pub outcomes: BTreeMap<String, Outcome>,
    pub changes: BTreeMap<String, ChangeRequest>,
    pub risks: BTreeMap<String, Risk>,
    pub incidents: BTreeMap<String, Incident>,
    pub closing_receipts: BTreeMap<String, CompanyClosingReceipt>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompanyEvent {
    pub schema: String,
    pub project_root: String,
    pub owner_id: String,
    pub request: CompanyCommandRequest,
    pub authority: CompanyAuthority,
    pub proof: CompanyProof,
}

impl CompanyState {
    /// Record a versioned cross-department handoff without acquiring a runtime lease.
    pub fn offer_accountability_handoff(
        &mut self,
        handoff: crate::CompanyHandoff,
    ) -> CompanyResult<()> {
        self.accountability_handoffs.offer(handoff)
    }

    /// Apply the recipient ACK/reject to the same state projection used by both departments.
    pub fn acknowledge_accountability_handoff(
        &mut self,
        handoff_id: &str,
        receiver: &crate::HandoffAssignment,
        accepted: bool,
        reason: impl Into<String>,
        evidence: crate::HandoffAcceptanceEvidence,
        now_ms: u64,
    ) -> CompanyResult<()> {
        self.accountability_handoffs
            .acknowledge(handoff_id, receiver, accepted, reason, evidence, now_ms)
    }

    /// Expiry is an explicit state transition so reopen/replay sees the same owner and escalation.
    pub fn expire_accountability_handoff(
        &mut self,
        handoff_id: &str,
        now_ms: u64,
    ) -> CompanyResult<()> {
        self.accountability_handoffs.expire(handoff_id, now_ms)
    }

    pub fn accountability_handoff_projection(
        &self,
        handoff_id: &str,
    ) -> Option<crate::HandoffProjection> {
        self.accountability_handoffs.projection(handoff_id)
    }

    /// Claim a short-lived attempt for a packet.  Repeating the same worker/request is idempotent;
    /// a different claimant cannot create a second active attempt.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_packet_attempt(
        &mut self,
        packet_id: impl Into<String>,
        packet_version: u64,
        worker_cell_id: crate::CellId,
        worker_session_id: crate::SessionId,
        execution_request_id: crate::RequestId,
        now_ms: u64,
        lease_expires_at: u64,
    ) -> CompanyResult<crate::PacketAttempt> {
        self.packet_attempts.claim(
            packet_id,
            packet_version,
            worker_cell_id,
            worker_session_id,
            execution_request_id,
            now_ms,
            lease_expires_at,
        )
    }

    pub fn fence_expired_packet_attempt(
        &mut self,
        packet_id: &str,
        attempt_id: crate::AttemptId,
        epoch: u64,
        now_ms: u64,
    ) -> CompanyResult<crate::PacketAttemptStatus> {
        self.packet_attempts
            .fence_expired(packet_id, attempt_id, epoch, now_ms)
    }

    pub fn mark_packet_attempt_started(
        &mut self,
        packet_id: &str,
        attempt_id: crate::AttemptId,
        epoch: u64,
        execution_request_id: crate::RequestId,
        now_ms: u64,
    ) -> CompanyResult<()> {
        self.packet_attempts.mark_started(
            packet_id,
            attempt_id,
            epoch,
            execution_request_id,
            now_ms,
        )
    }

    pub fn confirm_packet_attempt_stopped(
        &mut self,
        packet_id: &str,
        attempt_id: crate::AttemptId,
        epoch: u64,
        execution_request_id: crate::RequestId,
        now_ms: u64,
    ) -> CompanyResult<()> {
        self.packet_attempts.confirm_stopped(
            packet_id,
            attempt_id,
            epoch,
            execution_request_id,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_packet_attempt(
        &mut self,
        packet_id: &str,
        attempt_id: crate::AttemptId,
        epoch: u64,
        execution_request_id: crate::RequestId,
        status: crate::PacketAttemptStatus,
        result_digest: Option<String>,
        now_ms: u64,
    ) -> CompanyResult<()> {
        self.packet_attempts.complete(
            packet_id,
            attempt_id,
            epoch,
            execution_request_id,
            status,
            result_digest,
            now_ms,
        )
    }

    pub fn packet_attempt_history(&self, packet_id: &str) -> &[crate::PacketAttempt] {
        self.packet_attempts.history(packet_id)
    }

    /// Record a project control plan without claiming that child effects have stopped.
    pub fn record_project_control(
        &mut self,
        plan: crate::ProjectControlPlan,
    ) -> CompanyResult<()> {
        self.project_controls.record(plan)
    }

    pub fn latest_project_control(
        &self,
        project_id: &str,
    ) -> Option<&crate::ProjectControlPlan> {
        self.project_controls.latest(project_id)
    }

    pub fn record_company_risk_trigger(
        &mut self,
        trigger: crate::CompanyRiskTrigger,
    ) -> CompanyResult<()> {
        self.company_reconciliation.record_trigger(trigger)
    }

    pub fn open_company_incident(
        &mut self,
        incident: crate::CompanyIncident,
    ) -> CompanyResult<()> {
        self.company_reconciliation.open_incident(incident)
    }

    pub fn record_company_reconciliation(
        &mut self,
        case: crate::CompanyReconciliationCase,
    ) -> CompanyResult<()> {
        self.company_reconciliation.record_case(case)
    }

    pub fn publish_delivery_manifest(
        &mut self,
        manifest: crate::DeliveryManifest,
    ) -> CompanyResult<()> {
        self.delivery_manifests.publish_manifest(manifest)
    }

    pub fn record_local_delivery_package(
        &mut self,
        package: crate::LocalDeliveryPackage,
    ) -> CompanyResult<()> {
        self.delivery_manifests.record_package(package)
    }

    pub fn authorize_delivery(
        &mut self,
        authorization: crate::DeliveryAuthorization,
        manifest: &crate::DeliveryManifest,
        now: u64,
    ) -> CompanyResult<()> {
        self.delivery_authorizations
            .authorize(authorization, manifest, now)
    }

    pub fn request_delivery_dispatch(
        &mut self,
        intent: crate::DeliveryDispatchIntent,
        manifest: &crate::DeliveryManifest,
        now: u64,
    ) -> CompanyResult<()> {
        self.delivery_authorizations
            .request_dispatch(intent, manifest, now)
    }

    pub fn record_delivery_effect(
        &mut self,
        receipt: crate::DeliveryEffectReceipt,
        manifest: &crate::DeliveryManifest,
        package: &crate::LocalDeliveryPackage,
    ) -> CompanyResult<()> {
        self.delivery_authorizations
            .record_effect(receipt, manifest, package)
    }

    pub fn confirm_delivery_recipient(
        &mut self,
        confirmation: crate::DeliveryRecipientConfirmation,
    ) -> CompanyResult<()> {
        self.delivery_authorizations
            .confirm_recipient(confirmation)
    }

    pub fn record_closing_receipt(
        &mut self,
        receipt: crate::CompanyClosingReceiptContract,
    ) -> CompanyResult<()> {
        self.closing_receipt_contracts.record(receipt)
    }

    /// Project Company readiness from one explicit snapshot and clock.  The projection is
    /// read-only: it never acquires a claim, lease, budget or write scope.
    pub fn project_company_readiness(
        &self,
        project_id: &str,
        now_ms: u64,
    ) -> Result<crate::CompanyReadiness, crate::PacketGraphError> {
        let project_active = self.projects.get(project_id).map(|project| {
            matches!(
                project.status,
                ProjectStatus::Planned | ProjectStatus::Active
            )
        });
        let packets = self.project_packets(project_id);
        let mut context = crate::CompanyReadinessContext {
            project_active,
            ..Default::default()
        };
        for (packet_id, packet) in &self.packets {
            if packet.project_id != project_id {
                continue;
            }
            context.packet_approved.insert(
                packet_id.clone(),
                matches!(
                    packet.packet.status,
                    WorkPacketStatus::Approved | WorkPacketStatus::Assigned
                ),
            );
            if let Some(blockers) = (!self.business_packet_blockers(packet_id, now_ms).is_empty())
                .then(|| self.business_packet_blockers(packet_id, now_ms))
            {
                context.extra_blockers.insert(packet_id.clone(), blockers);
            }
        }
        for handoff in self
            .handoffs
            .values()
            .filter(|handoff| packets.contains_key(&handoff.packet_id))
        {
            let status = match handoff.status {
                crate::HandoffStatus::Pending => crate::CompanyHandoffStatus::Pending,
                crate::HandoffStatus::Acknowledged => crate::CompanyHandoffStatus::Acknowledged,
                crate::HandoffStatus::Rejected => crate::CompanyHandoffStatus::Rejected,
            };
            context.handoffs.insert(handoff.packet_id.clone(), status);
        }
        for handoff in self
            .accountability_handoffs
            .handoffs
            .values()
            .filter(|handoff| packets.contains_key(&handoff.packet.packet.packet_id))
        {
            context
                .handoffs
                .insert(handoff.packet.packet.packet_id.clone(), handoff.status);
        }
        crate::company_ready_packets(&packets, now_ms, &context)
    }

    /// Canonical packet status is derived from the durable Company run observations.
    pub fn project_packets(&self, project_id: &str) -> BTreeMap<String, WorkPacket> {
        self.packets
            .iter()
            .filter(|(_, p)| p.project_id == project_id)
            .map(|(id, p)| {
                let mut packet = p.packet.clone();
                if let Some(run) = self.runs.get(id) {
                    packet.status = match run.status {
                        ExecutionStatus::Completed => WorkPacketStatus::Succeeded,
                        ExecutionStatus::Failed
                        | ExecutionStatus::Blocked
                        | ExecutionStatus::Denied
                        | ExecutionStatus::ResultUnknown => WorkPacketStatus::Failed,
                        ExecutionStatus::Cancelled => WorkPacketStatus::Cancelled,
                        ExecutionStatus::AwaitingApproval => WorkPacketStatus::AwaitingApproval,
                        _ => WorkPacketStatus::Running,
                    };
                }
                (id.clone(), packet)
            })
            .collect()
    }
    pub fn transition(
        &self,
        command: &CompanyCommand,
        authority: &CompanyAuthority,
        proof: &CompanyProof,
    ) -> CompanyResult<Self> {
        required(&authority.actor_id)?;
        ensure(
            command
                .allowed_roles()
                .contains(&authority.role_id.as_str()),
            "company_role_denied",
        )?;
        ensure(
            authority.now_ms > 0 && !authority.session_id.is_empty(),
            "company_authority_invalid",
        )?;
        self.business_legacy_guard(command, authority)?;
        let mut next = self.clone();
        next.apply(command, authority, proof)?;
        if let CompanyCommand::StartRun {
            packet_id,
            project_id,
            ..
        } = command
        {
            if let Some(baseline) = next.business.baselines.get(project_id) {
                let assignment = next
                    .business_assignment(
                        &authority.actor_id,
                        "builder",
                        project_id,
                        authority.now_ms,
                    )?
                    .clone();
                let owner = next
                    .packets
                    .get(packet_id)
                    .and_then(|p| p.packet.claim.as_ref())
                    .map(|c| c.owner)
                    .ok_or("business_run_role_instance_required")?;
                next.business.run_bindings.insert(
                    packet_id.clone(),
                    crate::BusinessRunBinding {
                        packet_id: packet_id.clone(),
                        assignment_id: assignment.assignment_id,
                        assignment_version: assignment.version,
                        baseline_version: baseline.version,
                        role_instance_id: owner.to_string(),
                        execution_request_id: authority.execution_request_id,
                    },
                );
            }
        }
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or("company_revision_exhausted")?;
        Ok(next)
    }
    fn project(&self, id: &str) -> CompanyResult<&Project> {
        self.projects.get(id).ok_or("company_project_not_found")
    }
    fn require_project_writable(&self, id: &str) -> CompanyResult<()> {
        ensure(
            matches!(
                self.project(id)?.status,
                ProjectStatus::Approved | ProjectStatus::Planned | ProjectStatus::Active
            ),
            "company_project_not_approved_or_active",
        )
    }
    fn set_project(&mut self, id: &str, status: ProjectStatus) -> CompanyResult<()> {
        let p = self
            .projects
            .get_mut(id)
            .ok_or("company_project_not_found")?;
        p.status = p.status.transition(status)?;
        p.version += 1;
        Ok(())
    }
    fn evidence(proof: &CompanyProof, refs: &[String]) -> CompanyResult<()> {
        list(refs)?;
        ensure(
            refs.iter().all(|r| proof.events.contains(r)),
            "company_evidence_unverified",
        )
    }
    fn insert_incident(&mut self, incident: &Incident, proof: &CompanyProof) -> CompanyResult<()> {
        incident.validate()?;
        Self::evidence(proof, &incident.evidence_refs)?;
        ensure(
            incident.status == IncidentStatus::Open
                && !self.incidents.contains_key(&incident.incident_id),
            "company_incident_exists_or_invalid",
        )?;
        if let Some(project) = &incident.project_id {
            self.project(project)?;
        }
        self.incidents
            .insert(incident.incident_id.clone(), incident.clone());
        Ok(())
    }
    pub(crate) fn apply(
        &mut self,
        c: &CompanyCommand,
        a: &CompanyAuthority,
        p: &CompanyProof,
    ) -> CompanyResult<()> {
        match c {
            CompanyCommand::Business { schema, action } => {
                ensure(
                    schema == crate::COMPANY_BUSINESS_SCHEMA,
                    "business_schema_unsupported",
                )?;
                self.apply_business(action, a, p)?;
            }
            CompanyCommand::RegisterArtifact {
                artifact_id,
                relative_path,
            } => {
                required(artifact_id)?;
                required(relative_path)?;
                ensure(
                    !self.artifacts.contains_key(artifact_id),
                    "company_artifact_already_registered",
                )?;
                let artifact = p
                    .artifact
                    .as_ref()
                    .ok_or("company_artifact_evidence_required")?;
                ensure(
                    artifact.artifact_id == *artifact_id
                        && artifact.relative_path == *relative_path
                        && !artifact.text.is_empty()
                        && artifact.text.len() <= 65_536,
                    "company_artifact_snapshot_invalid",
                )?;
                self.artifacts.insert(artifact_id.clone(), artifact.clone());
            }
            CompanyCommand::ProposeObjective { objective } => {
                objective.validate()?;
                ensure(
                    objective.status == ObjectiveStatus::Proposed
                        && objective.version == 1
                        && objective.owner_principal_id == a.actor_id,
                    "objective_initial_state_invalid",
                )?;
                ensure(
                    !self.objectives.contains_key(&objective.objective_id),
                    "objective_already_exists",
                )?;
                self.objectives
                    .insert(objective.objective_id.clone(), objective.clone());
            }
            CompanyCommand::DecideObjective {
                objective_id,
                approve,
            } => {
                let o = self
                    .objectives
                    .get_mut(objective_id)
                    .ok_or("objective_not_found")?;
                ensure(
                    o.owner_principal_id == a.actor_id,
                    "objective_owner_mismatch",
                )?;
                if *approve {
                    ensure(
                        o.measurement_method
                            .as_deref()
                            .is_some_and(|method| !method.trim().is_empty()),
                        "objective_measurement_method_required",
                    )?;
                }
                o.status = o.status.transition(if *approve {
                    ObjectiveStatus::Active
                } else {
                    ObjectiveStatus::Rejected
                })?;
                o.version += 1;
            }
            CompanyCommand::AchieveObjective { objective_id } => {
                ensure(
                    self.outcomes.values().any(|o| {
                        o.objective_id == *objective_id && o.status == OutcomeStatus::Realized
                    }),
                    "objective_outcome_measurement_required",
                )?;
                let o = self
                    .objectives
                    .get_mut(objective_id)
                    .ok_or("objective_not_found")?;
                o.status = o.status.transition(ObjectiveStatus::Achieved)?;
                o.version += 1;
            }
            CompanyCommand::SubmitInitiative { initiative } => {
                initiative.validate()?;
                ensure(
                    initiative.status == InitiativeStatus::Intake
                        && initiative.version == 1
                        && initiative.decision.is_none()
                        && initiative.project_id.is_none()
                        && initiative.sponsor_id == a.actor_id,
                    "initiative_initial_state_invalid",
                )?;
                ensure(
                    initiative.objective_refs.iter().all(|r| {
                        self.objectives.get(r).is_some_and(|objective| {
                            objective.organization_id == initiative.organization_id
                        })
                    }),
                    "initiative_objective_missing",
                )?;
                ensure(
                    !self.initiatives.contains_key(&initiative.initiative_id),
                    "initiative_already_exists",
                )?;
                self.initiatives
                    .insert(initiative.initiative_id.clone(), initiative.clone());
            }
            CompanyCommand::AdvanceInitiative {
                initiative_id,
                status,
                decision,
                project_id,
            } => {
                let current = self
                    .initiatives
                    .get(initiative_id)
                    .ok_or("initiative_not_found")?;
                if matches!(
                    status,
                    InitiativeStatus::Approved
                        | InitiativeStatus::Rejected
                        | InitiativeStatus::ConvertedToProject
                ) {
                    ensure(a.role_id == "sponsor", "company_role_denied")?;
                    required(decision.as_deref().ok_or("initiative_decision_required")?)?;
                } else {
                    ensure(
                        matches!(a.role_id.as_str(), "pm" | "sponsor"),
                        "company_role_denied",
                    )?;
                }
                if *status == InitiativeStatus::Approved {
                    ensure(
                        current.objective_refs.iter().all(|objective_id| {
                            self.objectives.get(objective_id).is_some_and(|objective| {
                                objective.organization_id == current.organization_id
                                    && objective.status == ObjectiveStatus::Active
                            })
                        }),
                        "initiative_objective_approval_required",
                    )?;
                }
                if *status == InitiativeStatus::ConvertedToProject {
                    let project =
                        self.project(project_id.as_deref().ok_or("initiative_project_required")?)?;
                    ensure(
                        project.organization_id == current.organization_id
                            && current
                                .objective_refs
                                .iter()
                                .all(|objective_id| project.objective_refs.contains(objective_id)),
                        "initiative_project_objective_ancestry_mismatch",
                    )?;
                }
                let i = self
                    .initiatives
                    .get_mut(initiative_id)
                    .ok_or("initiative_not_found")?;
                i.status = i.status.transition(*status)?;
                i.version += 1;
                if decision.is_some() {
                    i.decision = decision.clone();
                }
                if project_id.is_some() {
                    i.project_id = project_id.clone();
                }
            }
            CompanyCommand::ProposeProject { project } => {
                project.validate()?;
                ensure(
                    project.status == ProjectStatus::Proposed
                        && project.version == 1
                        && project.sponsor_id == a.actor_id
                        && project.milestone_refs.is_empty()
                        && project.decision_ref.is_none()
                        && project.incident_id.is_none()
                        && project.acceptance_id.is_none()
                        && project.closing_receipt_id.is_none(),
                    "project_initial_state_invalid",
                )?;
                ensure(
                    project.objective_refs.iter().all(|id| {
                        self.objectives.get(id).is_some_and(|o| {
                            o.organization_id == project.organization_id
                                && o.status == ObjectiveStatus::Active
                        })
                    }),
                    "project_objective_not_active",
                )?;
                ensure(
                    !self.projects.contains_key(&project.project_id),
                    "project_already_exists",
                )?;
                let charter_digest = project
                    .charter_ref
                    .strip_prefix("artifact:")
                    .and_then(|id| self.artifacts.get(id))
                    .map(|artifact| {
                        ensure(
                            artifact.registered_at > 0 && !artifact.text.trim().is_empty(),
                            "project_charter_artifact_required",
                        )?;
                        Ok::<_, &'static str>(charter_identity_digest(artifact))
                    })
                    .transpose()?;
                self.projects
                    .insert(project.project_id.clone(), project.clone());
                if let Some(digest) = charter_digest {
                    self.charter_digests
                        .insert(project.project_id.clone(), digest);
                }
            }
            CompanyCommand::StartChartering { project_id } => {
                let project = self.project(project_id)?.clone();
                let charter_id = project
                    .charter_ref
                    .strip_prefix("artifact:")
                    .ok_or("project_charter_artifact_required")?;
                let charter = self
                    .artifacts
                    .get(charter_id)
                    .ok_or("project_charter_artifact_required")?;
                ensure(
                    charter.registered_at > 0 && !charter.text.trim().is_empty(),
                    "project_charter_artifact_required",
                )?;
                self.charter_digests
                    .insert(project_id.clone(), charter_identity_digest(charter));
                self.set_project(project_id, ProjectStatus::Chartering)?
            }
            CompanyCommand::ApproveProject {
                project_id,
                decision_ref,
            }
            | CompanyCommand::RejectProject {
                project_id,
                decision_ref,
            } => {
                let project = self.project(project_id)?;
                if matches!(c, CompanyCommand::ApproveProject { .. }) {
                    ensure(
                        project.status == ProjectStatus::Chartering,
                        "project_charter_not_ready_for_approval",
                    )?;
                    ensure(
                        self.budgets.contains_key(project_id),
                        "project_budget_required",
                    )?;
                    ensure(project.sponsor_id == a.actor_id, "project_sponsor_mismatch")?;
                    ensure(
                        project.objective_refs.iter().all(|objective_id| {
                            self.objectives.get(objective_id).is_some_and(|objective| {
                                objective.organization_id == project.organization_id
                                    && objective.status == ObjectiveStatus::Active
                            })
                        }),
                        "project_objective_not_active",
                    )?;
                }
                let charter_id = project
                    .charter_ref
                    .strip_prefix("artifact:")
                    .ok_or("project_charter_artifact_required")?;
                let charter = self
                    .artifacts
                    .get(charter_id)
                    .ok_or("project_charter_artifact_required")?;
                ensure(
                    charter.registered_at > 0 && !charter.text.trim().is_empty(),
                    "project_charter_artifact_required",
                )?;
                if matches!(c, CompanyCommand::ApproveProject { .. }) {
                    let charter_digest = charter_identity_digest(charter);
                    if let Some(expected_digest) = self.charter_digests.get(project_id) {
                        ensure(
                            expected_digest == &charter_digest,
                            "project_charter_changed",
                        )?;
                    }
                    if let Some(version) = charter.typed_version.as_ref() {
                        ensure(
                            version.validate().is_ok()
                                && version.content_hash == journal_sha256(charter.text.as_bytes()),
                            "project_charter_changed",
                        )?;
                    }
                }
                Self::evidence(p, &[project.charter_ref.clone(), decision_ref.clone()])?;
                let status = if matches!(c, CompanyCommand::ApproveProject { .. }) {
                    ProjectStatus::Approved
                } else {
                    ProjectStatus::Rejected
                };
                self.set_project(project_id, status)?;
                self.projects.get_mut(project_id).unwrap().decision_ref =
                    Some(decision_ref.clone());
                if matches!(c, CompanyCommand::ApproveProject { .. }) {
                    let approved_project = self
                        .projects
                        .get(project_id)
                        .ok_or("company_project_not_found")?;
                    let budget = self
                        .budgets
                        .get(project_id)
                        .ok_or("project_budget_required")?;
                    self.charter_baselines.insert(
                        project_id.clone(),
                        ProjectCharterBaseline::from_project(approved_project, budget),
                    );
                }
            }
            CompanyCommand::CreateMilestone { milestone } => {
                milestone.validate()?;
                self.require_project_writable(&milestone.project_id)?;
                ensure(
                    milestone.status == MilestoneStatus::Planned && milestone.version == 1,
                    "milestone_initial_state_invalid",
                )?;
                ensure(
                    !self.milestones.contains_key(&milestone.milestone_id),
                    "milestone_already_exists",
                )?;
                let project = self.project(&milestone.project_id)?;
                ensure(
                    milestone
                        .objective_refs
                        .iter()
                        .all(|id| project.objective_refs.contains(id)),
                    "milestone_objective_mismatch",
                )?;
                ensure(
                    milestone.dependency_refs.iter().all(|id| {
                        self.milestones
                            .get(id)
                            .is_some_and(|m| m.project_id == milestone.project_id)
                    }),
                    "milestone_dependency_missing",
                )?;
                self.milestones
                    .insert(milestone.milestone_id.clone(), milestone.clone());
                let project = self.projects.get_mut(&milestone.project_id).unwrap();
                project.milestone_refs.push(milestone.milestone_id.clone());
                project.version += 1;
            }
            CompanyCommand::ApprovePacket {
                project_id,
                milestone_id,
                packet,
            } => {
                self.require_project_writable(project_id)?;
                packet.validate()?;
                ensure(
                    packet.status == WorkPacketStatus::Draft
                        && !self.packets.contains_key(&packet.id),
                    "company_packet_exists_or_not_draft",
                )?;
                ensure(
                    !packet.path_allow.is_empty()
                        && packet
                            .path_allow
                            .iter()
                            .all(|path| crate::normalize_role_path(path).is_some()),
                    "company_packet_write_set_required",
                )?;
                ensure(
                    packet.claim.is_none()
                        && packet.owner_cell_id.is_none()
                        && packet.budget_lease_id.is_none()
                        && packet.acceptor_id.is_none()
                        && packet
                            .project_id
                            .is_none_or(|id| id.to_string() == *project_id),
                    "company_packet_authority_mismatch",
                )?;
                let criteria = if packet.acceptance.is_empty() {
                    &packet.acceptance_tests
                } else {
                    &packet.acceptance
                };
                list(criteria)?;
                let milestone = self
                    .milestones
                    .get(milestone_id)
                    .ok_or("milestone_not_found")?;
                ensure(
                    milestone.project_id == *project_id
                        && matches!(
                            milestone.status,
                            MilestoneStatus::Planned | MilestoneStatus::Active
                        ),
                    "company_packet_milestone_mismatch",
                )?;
                ensure(
                    packet.dependencies.iter().all(|id| {
                        self.packets
                            .get(id)
                            .is_some_and(|p| p.project_id == *project_id)
                    }),
                    "company_packet_dependency_missing",
                )?;
                // Validate the graph including the candidate before appending it.  Existence
                // checks alone would allow a new edge to close a cycle after the packet was
                // persisted; the domain graph helper is deterministic and fail-closed.
                let mut graph = self.project_packets(project_id);
                graph.insert(packet.id.clone(), packet.clone());
                crate::validate_dependency_dag(&graph)
                    .map_err(|_| "company_packet_dependency_graph_invalid")?;
                if a.execution_cell_id.is_some() {
                    let handoff_id = format!("packet:{}", packet.id);
                    self.handoffs.insert(
                        handoff_id.clone(),
                        crate::PacketHandoff {
                            handoff_id,
                            packet_id: packet.id.clone(),
                            packet_version: 1,
                            from_department: "planning".into(),
                            from_role: a.role_id.clone(),
                            from_session: a.session_id.clone(),
                            to_department: "executing".into(),
                            to_role: "builder".into(),
                            created_at: a.now_ms,
                            expires_at: packet
                                .deadline_unix_ms
                                .unwrap_or(a.now_ms.saturating_add(86_400_000)),
                            status: crate::HandoffStatus::Pending,
                            acknowledgement: None,
                        },
                    );
                }
                let mut approved = packet.clone();
                approved.status = approved
                    .status
                    .transition(WorkPacketStatus::Approved)
                    .map_err(|_| "company_packet_state_invalid")?;
                self.packets.insert(
                    packet.id.clone(),
                    CompanyPacket {
                        project_id: project_id.clone(),
                        milestone_id: milestone_id.clone(),
                        packet: approved,
                        version: 1,
                    },
                );
            }
            CompanyCommand::ReworkPacket {
                packet_id,
                replacement,
            } => {
                replacement.validate()?;
                let original = self
                    .packets
                    .get(packet_id)
                    .ok_or("company_packet_not_found")?
                    .clone();
                let project = self.project(&original.project_id)?;
                ensure(
                    project.status == ProjectStatus::Active
                        && replacement.status == WorkPacketStatus::Draft
                        && replacement.id != *packet_id
                        && !self.packets.contains_key(&replacement.id),
                    "company_rework_packet_invalid",
                )?;
                ensure(
                    replacement.claim.is_none()
                        && replacement.owner_cell_id.is_none()
                        && replacement.budget_lease_id.is_none()
                        && replacement.acceptor_id.is_none()
                        && replacement
                            .project_id
                            .is_none_or(|id| id.to_string() == original.project_id),
                    "company_packet_authority_mismatch",
                )?;
                let old_criteria = if original.packet.acceptance.is_empty() {
                    &original.packet.acceptance_tests
                } else {
                    &original.packet.acceptance
                };
                let new_criteria = if replacement.acceptance.is_empty() {
                    &replacement.acceptance_tests
                } else {
                    &replacement.acceptance
                };
                ensure(
                    old_criteria == new_criteria
                        && original.packet.path_allow == replacement.path_allow
                        && replacement.dependencies == original.packet.dependencies,
                    "company_rework_baseline_changed",
                )?;
                let milestone = self
                    .milestones
                    .get_mut(&original.milestone_id)
                    .ok_or("milestone_not_found")?;
                milestone.status = milestone.status.transition(MilestoneStatus::Rework)?;
                milestone.version += 1;
                if a.execution_cell_id.is_some() {
                    let handoff_id = format!("packet:{}", replacement.id);
                    self.handoffs.insert(
                        handoff_id.clone(),
                        crate::PacketHandoff {
                            handoff_id,
                            packet_id: replacement.id.clone(),
                            packet_version: 1,
                            from_department: "planning".into(),
                            from_role: a.role_id.clone(),
                            from_session: a.session_id.clone(),
                            to_department: "executing".into(),
                            to_role: "builder".into(),
                            created_at: a.now_ms,
                            expires_at: replacement
                                .deadline_unix_ms
                                .unwrap_or(a.now_ms.saturating_add(86_400_000)),
                            status: crate::HandoffStatus::Pending,
                            acknowledgement: None,
                        },
                    );
                }
                let mut packet = replacement.clone();
                packet.status = WorkPacketStatus::Approved;
                self.packets.insert(
                    packet.id.clone(),
                    CompanyPacket {
                        project_id: original.project_id,
                        milestone_id: original.milestone_id,
                        packet,
                        version: 1,
                    },
                );
            }
            CompanyCommand::PlanProject { project_id } => {
                crate::validate_dependency_dag(&self.project_packets(project_id))
                    .map_err(|_| "company_packet_graph_invalid")?;
                ensure(
                    self.packets
                        .values()
                        .any(|packet| packet.project_id == *project_id),
                    "project_frozen_packet_required",
                )?;
                self.set_project(project_id, ProjectStatus::Planned)?;
            }
            CompanyCommand::AcknowledgeHandoff {
                handoff_id,
                accept,
                reason,
            } => {
                let handoff = self
                    .handoffs
                    .get_mut(handoff_id)
                    .ok_or("handoff_not_found")?;
                handoff.acknowledge(a, *accept, reason)?;
            }
            CompanyCommand::ReviewPacket {
                packet_id,
                review_id,
                criterion_results,
                evidence_refs,
            } => {
                let packet = self
                    .packets
                    .get(packet_id)
                    .ok_or("company_packet_not_found")?;
                ensure(
                    !self.packet_reviews.contains_key(review_id) && !review_id.trim().is_empty(),
                    "packet_review_identity_invalid",
                )?;
                let run = p.run.as_ref().ok_or("company_run_evidence_required")?;
                ensure(
                    run.packet_id == *packet_id
                        && run.status == ExecutionStatus::Completed
                        && run.author_session_id != a.session_id,
                    "packet_review_run_or_session_invalid",
                )?;
                let criteria = if packet.packet.acceptance.is_empty() {
                    &packet.packet.acceptance_tests
                } else {
                    &packet.packet.acceptance
                };
                let mut expected = criteria.clone();
                expected.sort();
                expected.dedup();
                ensure(
                    !expected.is_empty()
                        && criterion_results.keys().cloned().collect::<Vec<_>>() == expected,
                    "packet_review_criteria_mismatch",
                )?;
                Self::evidence(p, evidence_refs)?;
                self.packet_reviews.insert(
                    review_id.clone(),
                    crate::PacketReview {
                        review_id: review_id.clone(),
                        packet_id: packet_id.clone(),
                        packet_version: packet.version,
                        author_run_id: run.run_id.ok_or("company_run_id_missing")?,
                        author_session_id: run.author_session_id.clone(),
                        reviewer_session_id: a.session_id.clone(),
                        criterion_results: criterion_results.clone(),
                        evidence_refs: evidence_refs.clone(),
                        reviewed_at: a.now_ms,
                    },
                );
            }
            CompanyCommand::ConfigureBudget { project_id, policy } => {
                let project = self.project(project_id)?;
                ensure(
                    matches!(
                        project.status,
                        ProjectStatus::Proposed | ProjectStatus::Chartering
                    ),
                    "company_project_not_budgetable",
                )?;
                ensure(
                    !self.runs.values().any(|r| r.project_id == *project_id),
                    "company_budget_already_reserved",
                )?;
                ensure(
                    (policy.project.project_id.to_string() == *project_id
                        || policy.quota.scope == *project_id)
                        && policy.quota.scope == *project_id,
                    "company_budget_scope_mismatch",
                )?;
                ensure(
                    !self.budgets.contains_key(project_id),
                    "company_budget_already_configured",
                )?;
                policy.runtime.validate()?;
                policy
                    .project
                    .check_reservation(0, 0, policy.runtime.max_tokens)?;
                policy.quota.check_reservation(
                    policy.runtime.max_model_calls,
                    policy.runtime.max_tokens,
                    1,
                )?;
                self.budgets.insert(project_id.clone(), policy.clone());
            }
            CompanyCommand::ClaimPacket { packet_id } => {
                let packet = self
                    .packets
                    .get(packet_id)
                    .ok_or("company_packet_not_found")?;
                let project_id = packet.project_id.clone();
                ensure(
                    matches!(
                        self.project(&project_id)?.status,
                        ProjectStatus::Planned | ProjectStatus::Active
                    ),
                    "project_not_active",
                )?;
                let ready = crate::ready_packets(&self.project_packets(&project_id), a.now_ms)
                    .map_err(|_| "company_packet_graph_invalid")?;
                ensure(
                    ready.ready.contains(packet_id) && packet.packet.claim.is_none(),
                    "company_packet_not_ready",
                )?;
                let packet = &mut self.packets.get_mut(packet_id).unwrap().packet;
                packet.claim = Some(crate::PacketClaim {
                    owner: a.execution_cell_id.ok_or("company_claim_cell_required")?,
                    owner_session_id: a.session_id.clone(),
                    heartbeat_at: a.now_ms,
                    lease_expires_at: a
                        .now_ms
                        .saturating_add(crate::PACKET_LEASE_TTL_MS)
                        .min(packet.deadline_unix_ms.unwrap_or(u64::MAX)),
                });
            }
            CompanyCommand::RenewPacketClaim { packet_id } => {
                let packet = self
                    .packets
                    .get_mut(packet_id)
                    .ok_or("company_packet_not_found")?;
                let claim = packet
                    .packet
                    .claim
                    .as_mut()
                    .ok_or("company_packet_not_claimed")?;
                ensure(
                    claim.owner_session_id == a.session_id,
                    "company_claim_session_mismatch",
                )?;
                claim.renew(
                    a.execution_cell_id.ok_or("company_claim_cell_required")?,
                    a.now_ms,
                    packet.packet.deadline_unix_ms,
                )?;
            }
            CompanyCommand::ReclaimPacketClaim { packet_id } => {
                let packet = self
                    .packets
                    .get_mut(packet_id)
                    .ok_or("company_packet_not_found")?;
                let claim = packet
                    .packet
                    .claim
                    .as_ref()
                    .ok_or("company_packet_not_claimed")?;
                ensure(!claim.active(a.now_ms), "company_claim_not_expired")?;
                // Only work that was never dispatched becomes ready again. An expired in-flight
                // lease is an observation gap, never permission to repeat its side effects.
                if let Some(previous) = self.runs.get_mut(packet_id) {
                    let observed = p.run.as_ref().ok_or("company_run_evidence_required")?;
                    ensure(
                        observed.execution_request_id == previous.execution_request_id,
                        "company_run_identity_mismatch",
                    )?;
                    ensure(
                        observed.status.is_terminal(),
                        "company_claim_worker_not_stopped",
                    )?;
                    *previous = observed.clone();
                    if observed.status == ExecutionStatus::ResultUnknown {
                        let incident_id =
                            format!("lease-unknown-{}", observed.execution_request_id);
                        self.incidents
                            .entry(incident_id.clone())
                            .or_insert(Incident {
                                incident_id: incident_id.clone(),
                                project_id: Some(observed.project_id.clone()),
                                run_id: observed.run_id,
                                risk_id: None,
                                delivery_id: None,
                                severity: "error".into(),
                                detected_at: a.now_ms,
                                impact:
                                    "Expired worker lease has an unknown result; retry is forbidden"
                                        .into(),
                                timeline: vec!["lease_expired".into()],
                                owner_id: a.actor_id.clone(),
                                response_actions: Vec::new(),
                                evidence_refs: observed.evidence_refs.clone(),
                                escalation_target: None,
                                status: IncidentStatus::Open,
                            });
                        previous.incident_id = Some(incident_id);
                    }
                }
                packet.packet.claim = None;
            }
            CompanyCommand::StartRun {
                project_id,
                packet_id,
                ..
            } => {
                ensure(a.role_id == "builder", "company_role_denied")?;
                if let Some(policy) = self.budgets.get(project_id) {
                    let runs = self
                        .runs
                        .values()
                        .filter(|r| r.project_id == *project_id)
                        .count() as u64;
                    let active = self
                        .runs
                        .values()
                        .filter(|r| r.project_id == *project_id && !r.status.is_terminal())
                        .count() as u32;
                    policy.project.check_reservation(
                        runs,
                        runs.saturating_mul(policy.runtime.max_tokens),
                        policy.runtime.max_tokens,
                    )?;
                    policy.quota.check_reservation(
                        runs.saturating_add(1)
                            .saturating_mul(policy.runtime.max_model_calls),
                        runs.saturating_add(1)
                            .saturating_mul(policy.runtime.max_tokens),
                        active.saturating_add(1),
                    )?;
                }
                let project = self.project(project_id)?;
                ensure(
                    matches!(
                        project.status,
                        ProjectStatus::Planned | ProjectStatus::Active
                    ),
                    "project_not_planned",
                )?;
                ensure(
                    !self.runs.contains_key(packet_id),
                    "company_packet_already_dispatched",
                )?;
                let packet = self
                    .packets
                    .get(packet_id)
                    .ok_or("company_packet_not_found")?;
                ensure(
                    packet.project_id == *project_id
                        && packet.packet.status == WorkPacketStatus::Approved,
                    "company_packet_not_approved",
                )?;
                let mut graph = self.project_packets(project_id);
                if let Some(claim) = &packet.packet.claim {
                    ensure(
                        claim.active(a.now_ms) && claim.owner_session_id == a.session_id,
                        "company_packet_claim_conflict",
                    )?;
                    graph.get_mut(packet_id).unwrap().claim = None;
                }
                let ready = crate::ready_packets(&graph, a.now_ms)
                    .map_err(|_| "company_packet_graph_invalid")?;
                ensure(
                    ready.ready.contains(packet_id),
                    "company_packet_dependencies_incomplete",
                )?;
                let milestone_id = packet.milestone_id.clone();
                let milestone = self
                    .milestones
                    .get(&milestone_id)
                    .ok_or("milestone_not_found")?;
                ensure(
                    milestone.dependency_refs.iter().all(|id| {
                        self.milestones.get(id).is_some_and(|m| {
                            matches!(
                                m.status,
                                MilestoneStatus::Accepted | MilestoneStatus::Closed
                            )
                        })
                    }),
                    "milestone_dependencies_incomplete",
                )?;
                if project.status == ProjectStatus::Planned {
                    self.set_project(project_id, ProjectStatus::Active)?;
                }
                let milestone = self.milestones.get_mut(&milestone_id).unwrap();
                if milestone.status == MilestoneStatus::Planned {
                    milestone.status = MilestoneStatus::Active;
                    milestone.version += 1;
                }
                if let Some(handoff) = self.handoffs.get_mut(&format!("packet:{packet_id}")) {
                    if handoff.status == crate::HandoffStatus::Pending {
                        handoff.acknowledge(
                            a,
                            true,
                            "Builder accepted the frozen packet at dispatch",
                        )?;
                    }
                    ensure(
                        handoff.status == crate::HandoffStatus::Acknowledged
                            && handoff
                                .acknowledgement
                                .as_ref()
                                .is_some_and(|ack| ack.receiver_session == a.session_id),
                        "handoff_ack_required",
                    )?;
                }
                let packet = &mut self.packets.get_mut(packet_id).unwrap().packet;
                if packet.claim.is_none() {
                    if let Some(owner) = a.execution_cell_id {
                        packet.claim = Some(crate::PacketClaim {
                            owner,
                            owner_session_id: a.session_id.clone(),
                            heartbeat_at: a.now_ms,
                            lease_expires_at: a
                                .now_ms
                                .saturating_add(crate::PACKET_LEASE_TTL_MS)
                                .min(packet.deadline_unix_ms.unwrap_or(u64::MAX)),
                        });
                    }
                }
                self.runs.insert(
                    packet_id.clone(),
                    CompanyRun {
                        packet_id: packet_id.clone(),
                        project_id: project_id.clone(),
                        execution_request_id: a.execution_request_id,
                        author_session_id: a.session_id.clone(),
                        run_id: None,
                        status: ExecutionStatus::Accepted,
                        evidence_refs: Vec::new(),
                        runtime_receipt: None,
                        incident_id: None,
                    },
                );
            }
            CompanyCommand::ReconcileRun { packet_id }
            | CompanyCommand::RecordRunStarted { packet_id } => {
                let previous = self.runs.get(packet_id).ok_or("company_run_not_reserved")?;
                let observed = p.run.as_ref().ok_or("company_run_evidence_required")?;
                if matches!(c, CompanyCommand::RecordRunStarted { .. }) {
                    ensure(
                        previous.run_id.is_none() && observed.run_id.is_some(),
                        "company_run_already_started_or_unobserved",
                    )?;
                }
                ensure(
                    observed.packet_id == *packet_id
                        && observed.execution_request_id == previous.execution_request_id
                        && observed.author_session_id == previous.author_session_id
                        && observed.project_id == previous.project_id,
                    "company_run_identity_mismatch",
                )?;
                if previous.status.is_terminal() {
                    ensure(
                        previous.status == observed.status && previous.run_id == observed.run_id,
                        "company_run_terminal_conflict",
                    )?;
                }
                Self::evidence(p, &observed.evidence_refs)?;
                if observed.status == ExecutionStatus::ResultUnknown {
                    let incident_id = format!("run-unknown-{}", observed.execution_request_id);
                    if !self.incidents.contains_key(&incident_id) {
                        self.incidents.insert(incident_id.clone(),Incident{incident_id:incident_id.clone(),project_id:Some(observed.project_id.clone()),run_id:observed.run_id,risk_id:None,delivery_id:None,severity:"error".to_owned(),detected_at:a.now_ms,impact:"Run result requires reconciliation; automatic retry is forbidden".to_owned(),timeline:vec!["result_unknown".to_owned()],owner_id:a.actor_id.clone(),response_actions:Vec::new(),evidence_refs:observed.evidence_refs.clone(),escalation_target:None,status:IncidentStatus::Open});
                    }
                    let mut observed = observed.clone();
                    observed.incident_id = Some(incident_id);
                    self.runs.insert(packet_id.clone(), observed);
                } else {
                    self.runs.insert(packet_id.clone(), observed.clone());
                }
            }
            CompanyCommand::RequestAcceptance {
                acceptance_id,
                project_id,
                packet_id,
                evidence_refs,
            } => {
                required(acceptance_id)?;
                ensure(
                    !self.acceptances.contains_key(acceptance_id),
                    "acceptance_already_exists",
                )?;
                Self::evidence(p, evidence_refs)?;
                let project = self.project(project_id)?.clone();
                ensure(
                    project.status == ProjectStatus::Active,
                    "project_not_active",
                )?;
                let packet = self
                    .packets
                    .get(packet_id)
                    .ok_or("company_packet_not_found")?
                    .clone();
                ensure(
                    packet.project_id == *project_id,
                    "acceptance_packet_project_mismatch",
                )?;
                let observed = p
                    .run
                    .as_ref()
                    .or_else(|| self.runs.get(packet_id))
                    .ok_or("company_run_evidence_required")?
                    .clone();
                ensure(
                    observed.packet_id == *packet_id
                        && observed.status == ExecutionStatus::Completed,
                    "acceptance_run_not_completed",
                )?;
                ensure(
                    runtime_receipt_matches(observed, &observed.evidence_refs),
                    "acceptance_runtime_receipt_required",
                )?;
                ensure(
                    self.runs
                        .iter()
                        .filter(|(_, r)| r.project_id == *project_id && r.packet_id != *packet_id)
                        .all(|(_, r)| r.status == ExecutionStatus::Completed),
                    "project_other_runs_incomplete",
                )?;
                ensure(
                    self.packets
                        .values()
                        .filter(|p| p.project_id == *project_id)
                        .all(|p| {
                            p.packet.id == *packet_id
                                || self
                                    .runs
                                    .get(&p.packet.id)
                                    .is_some_and(|r| r.status == ExecutionStatus::Completed)
                        }),
                    "project_packets_incomplete",
                )?;
                let milestone = self
                    .milestones
                    .get(&packet.milestone_id)
                    .ok_or("milestone_not_found")?
                    .clone();
                let snapshot = CriteriaSnapshot {
                    project_version: project.version,
                    milestone_version: milestone.version,
                    packet_version: packet.version,
                    milestone_versions: self
                        .milestones
                        .values()
                        .filter(|m| m.project_id == *project_id)
                        .map(|m| (m.milestone_id.clone(), m.version))
                        .collect(),
                    packet_versions: self
                        .packets
                        .values()
                        .filter(|p| p.project_id == *project_id)
                        .map(|p| (p.packet.id.clone(), p.version))
                        .collect(),
                    project_criteria: project.success_criteria.clone(),
                    milestone_criteria: self
                        .milestones
                        .values()
                        .filter(|m| m.project_id == *project_id)
                        .flat_map(|m| m.acceptance_criteria.clone())
                        .collect(),
                    packet_criteria: self
                        .packets
                        .values()
                        .filter(|p| p.project_id == *project_id)
                        .flat_map(|p| {
                            if p.packet.acceptance.is_empty() {
                                p.packet.acceptance_tests.clone()
                            } else {
                                p.packet.acceptance.clone()
                            }
                        })
                        .collect(),
                    criterion_refs: Vec::new(),
                };
                let mut refs = evidence_refs.clone();
                refs.extend(observed.evidence_refs.clone());
                for run in self.runs.values().filter(|r| r.project_id == *project_id) {
                    refs.extend(run.evidence_refs.clone());
                }
                refs.sort();
                refs.dedup();
                self.acceptances.insert(
                    acceptance_id.clone(),
                    Acceptance {
                        acceptance_id: acceptance_id.clone(),
                        project_id: project_id.clone(),
                        milestone_id: packet.milestone_id.clone(),
                        work_packet_id: packet_id.clone(),
                        criteria_snapshot: snapshot,
                        evidence_refs: refs,
                        author_run_id: observed.run_id.ok_or("acceptance_run_id_required")?,
                        author_session_id: observed.author_session_id.clone(),
                        reviewer_id: None,
                        decision_maker_id: None,
                        decision: None,
                        decided_at: None,
                        rejection_reasons: Vec::new(),
                        status: AcceptanceStatus::Requested,
                        version: 1,
                    },
                );
                self.runs.insert(packet_id.clone(), observed);
                self.set_project(project_id, ProjectStatus::ReadyForAcceptance)?;
                self.projects.get_mut(project_id).unwrap().acceptance_id =
                    Some(acceptance_id.clone());
                for m in self
                    .milestones
                    .values_mut()
                    .filter(|m| m.project_id == *project_id)
                {
                    m.status = m.status.transition(MilestoneStatus::ReadyForAcceptance)?;
                    m.version += 1;
                }
            }
            CompanyCommand::RecordReview {
                review_id,
                acceptance_id,
                criterion_results,
                evidence_refs,
            } => {
                required(review_id)?;
                Self::evidence(p, evidence_refs)?;
                ensure(
                    !self.reviews.contains_key(review_id),
                    "review_already_exists",
                )?;
                let acceptance_view = self
                    .acceptances
                    .get(acceptance_id)
                    .ok_or("acceptance_not_found")?;
                let author_run = self
                    .runs
                    .values()
                    .find(|run| run.run_id == Some(acceptance_view.author_run_id))
                    .ok_or("acceptance_runtime_run_missing")?;
                ensure(
                    runtime_receipt_matches(author_run, &acceptance_view.evidence_refs),
                    "review_runtime_receipt_required",
                )?;
                let acceptance = self
                    .acceptances
                    .get_mut(acceptance_id)
                    .ok_or("acceptance_not_found")?;
                ensure(
                    acceptance.status == AcceptanceStatus::Requested,
                    "acceptance_not_awaiting_review",
                )?;
                ensure(
                    a.session_id != acceptance.author_session_id,
                    "review_author_session_denied",
                )?;
                ensure(
                    criterion_results.keys().cloned().collect::<Vec<_>>()
                        == acceptance.criteria_snapshot.criteria(),
                    "review_criteria_snapshot_mismatch",
                )?;
                ensure(
                    evidence_refs
                        .iter()
                        .any(|r| acceptance.evidence_refs.contains(r)),
                    "review_author_evidence_required",
                )?;
                self.reviews.insert(
                    review_id.clone(),
                    CompanyReview {
                        review_id: review_id.clone(),
                        acceptance_id: acceptance_id.clone(),
                        author_run_id: acceptance.author_run_id,
                        reviewer_session_id: a.session_id.clone(),
                        reviewer_id: a.actor_id.clone(),
                        criteria_snapshot: acceptance.criteria_snapshot.clone(),
                        criterion_results: criterion_results.clone(),
                        evidence_refs: evidence_refs.clone(),
                        recorded_at: a.now_ms,
                    },
                );
                acceptance.reviewer_id = Some(a.actor_id.clone());
                acceptance.status = acceptance
                    .status
                    .transition(AcceptanceStatus::EvidencePending)?
                    .transition(AcceptanceStatus::ReadyForDecision)?;
                acceptance.version += 1;
            }
            CompanyCommand::DecideAcceptance {
                acceptance_id,
                review_id,
                decision,
                reasons,
                waiver_ref,
            } => {
                let review = self
                    .reviews
                    .get(review_id)
                    .ok_or("acceptance_independent_review_required")?
                    .clone();
                let acceptance = self
                    .acceptances
                    .get(acceptance_id)
                    .ok_or("acceptance_not_found")?
                    .clone();
                ensure(
                    review.acceptance_id == *acceptance_id
                        && review.criteria_snapshot == acceptance.criteria_snapshot
                        && review.author_run_id == acceptance.author_run_id
                        && a.session_id != acceptance.author_session_id,
                    "acceptance_review_identity_mismatch",
                )?;
                ensure(
                    a.role_id == "sponsor" || a.session_id == review.reviewer_session_id,
                    "acceptance_reviewer_session_mismatch",
                )?;
                let (status, project_status, milestone_status) = match decision {
                    AcceptanceDecision::Accept => {
                        ensure(
                            review.criterion_results.values().all(|v| *v),
                            "acceptance_criteria_failed",
                        )?;
                        (
                            AcceptanceStatus::Accepted,
                            ProjectStatus::Accepted,
                            MilestoneStatus::Accepted,
                        )
                    }
                    AcceptanceDecision::Reject => {
                        list(reasons)?;
                        (
                            AcceptanceStatus::Rejected,
                            ProjectStatus::Active,
                            MilestoneStatus::Rejected,
                        )
                    }
                    AcceptanceDecision::Waive => {
                        ensure(a.role_id == "sponsor", "acceptance_waiver_requires_sponsor")?;
                        Self::evidence(
                            p,
                            &[waiver_ref
                                .clone()
                                .ok_or("acceptance_waiver_evidence_required")?],
                        )?;
                        list(reasons)?;
                        (
                            AcceptanceStatus::Waived,
                            ProjectStatus::Closed,
                            MilestoneStatus::Accepted,
                        )
                    }
                };
                let target = self.acceptances.get_mut(acceptance_id).unwrap();
                target.status = target.status.transition(status)?;
                target.version += 1;
                target.decision = Some(*decision);
                target.decision_maker_id = Some(a.actor_id.clone());
                target.decided_at = Some(a.now_ms);
                target.rejection_reasons = reasons.clone();
                self.set_project(&acceptance.project_id, project_status)?;
                for m in self
                    .milestones
                    .values_mut()
                    .filter(|m| m.project_id == acceptance.project_id)
                {
                    m.status = m.status.transition(milestone_status)?;
                    m.version += 1;
                }
            }
            CompanyCommand::PrepareDelivery { delivery } => {
                delivery.validate()?;
                ensure(
                    delivery.artifact_refs.iter().all(|r| {
                        r.strip_prefix("artifact:")
                            .is_some_and(|id| self.artifacts.contains_key(id))
                    }),
                    "delivery_artifact_snapshot_required",
                )?;
                Self::evidence(p, &delivery.artifact_refs)?;
                ensure(
                    delivery.status == DeliveryStatus::Prepared
                        && delivery.version == 1
                        && delivery.delivered_at.is_none()
                        && delivery.handoff_receipt_ref.is_none()
                        && delivery.incident_id.is_none(),
                    "delivery_initial_state_invalid",
                )?;
                let acceptance = self
                    .acceptances
                    .get(&delivery.acceptance_id)
                    .ok_or("acceptance_not_found")?;
                let author_run = self
                    .runs
                    .values()
                    .find(|run| run.run_id == Some(acceptance.author_run_id))
                    .ok_or("delivery_runtime_run_missing")?;
                ensure(
                    runtime_receipt_matches(author_run, &acceptance.evidence_refs),
                    "delivery_runtime_receipt_required",
                )?;
                ensure(
                    acceptance.project_id == delivery.project_id
                        && acceptance.status == AcceptanceStatus::Accepted,
                    "delivery_acceptance_required",
                )?;
                ensure(
                    !self.deliveries.contains_key(&delivery.delivery_id)
                        && !self.deliveries.values().any(|d| {
                            d.acceptance_id == delivery.acceptance_id
                                && d.status != DeliveryStatus::Failed
                        }),
                    "delivery_already_exists",
                )?;
                self.deliveries
                    .insert(delivery.delivery_id.clone(), delivery.clone());
            }
            CompanyCommand::ApproveDelivery { delivery_id } => {
                let d = self
                    .deliveries
                    .get_mut(delivery_id)
                    .ok_or("delivery_not_found")?;
                d.status = d.status.transition(DeliveryStatus::Approved)?;
                d.version += 1;
            }
            CompanyCommand::Deliver {
                delivery_id,
                evidence_refs,
            } => {
                Self::evidence(p, evidence_refs)?;
                let delivery_view = self
                    .deliveries
                    .get(delivery_id)
                    .ok_or("delivery_not_found")?;
                let acceptance = self
                    .acceptances
                    .get(&delivery_view.acceptance_id)
                    .ok_or("acceptance_not_found")?;
                let author_run = self
                    .runs
                    .values()
                    .find(|run| run.run_id == Some(acceptance.author_run_id))
                    .ok_or("delivery_runtime_run_missing")?;
                ensure(
                    runtime_receipt_matches(author_run, &acceptance.evidence_refs),
                    "delivery_runtime_receipt_required",
                )?;
                let d = self
                    .deliveries
                    .get_mut(delivery_id)
                    .ok_or("delivery_not_found")?;
                ensure(
                    d.artifact_refs.iter().all(|r| evidence_refs.contains(r)),
                    "delivery_artifact_evidence_required",
                )?;
                d.status = d.status.transition(DeliveryStatus::Delivered)?;
                d.delivered_at = Some(a.now_ms);
                d.version += 1;
            }
            CompanyCommand::ConfirmDelivery {
                delivery_id,
                handoff_receipt_ref,
            } => {
                Self::evidence(p, &[handoff_receipt_ref.clone()])?;
                let delivery_view = self
                    .deliveries
                    .get(delivery_id)
                    .ok_or("delivery_not_found")?;
                let acceptance = self
                    .acceptances
                    .get(&delivery_view.acceptance_id)
                    .ok_or("acceptance_not_found")?;
                let author_run = self
                    .runs
                    .values()
                    .find(|run| run.run_id == Some(acceptance.author_run_id))
                    .ok_or("delivery_runtime_run_missing")?;
                ensure(
                    runtime_receipt_matches(author_run, &acceptance.evidence_refs),
                    "delivery_runtime_receipt_required",
                )?;
                let d = self
                    .deliveries
                    .get_mut(delivery_id)
                    .ok_or("delivery_not_found")?;
                ensure(
                    d.recipient_ref == a.actor_id,
                    "delivery_recipient_ack_required",
                )?;
                d.status = d.status.transition(DeliveryStatus::Confirmed)?;
                d.handoff_receipt_ref = Some(handoff_receipt_ref.clone());
                d.version += 1;
            }
            CompanyCommand::MarkDeliveryUnknown {
                delivery_id,
                incident,
            } => {
                let d = self
                    .deliveries
                    .get(delivery_id)
                    .ok_or("delivery_not_found")?;
                ensure(
                    incident.delivery_id.as_deref() == Some(delivery_id)
                        && incident.project_id.as_deref() == Some(&d.project_id),
                    "delivery_incident_mismatch",
                )?;
                self.insert_incident(incident, p)?;
                let d = self.deliveries.get_mut(delivery_id).unwrap();
                d.status = d.status.transition(DeliveryStatus::DeliveryUnknown)?;
                d.incident_id = Some(incident.incident_id.clone());
                d.version += 1;
            }
            CompanyCommand::ReconcileDelivery {
                delivery_id,
                confirmed,
                evidence_refs,
            } => {
                Self::evidence(p, evidence_refs)?;
                let d = self
                    .deliveries
                    .get_mut(delivery_id)
                    .ok_or("delivery_not_found")?;
                ensure(
                    !*confirmed || d.recipient_ref == a.actor_id,
                    "delivery_recipient_ack_required",
                )?;
                d.status = d
                    .status
                    .transition(DeliveryStatus::Reconciled)?
                    .transition(if *confirmed {
                        DeliveryStatus::Confirmed
                    } else {
                        DeliveryStatus::Failed
                    })?;
                if *confirmed {
                    d.handoff_receipt_ref = Some(evidence_refs[0].clone());
                }
                d.version += 1;
            }
            CompanyCommand::CloseProject {
                project_id,
                delivery_id,
                receipt_id,
                evidence_refs,
            } => {
                required(receipt_id)?;
                Self::evidence(p, evidence_refs)?;
                ensure(
                    !self.closing_receipts.contains_key(receipt_id),
                    "closing_receipt_already_exists",
                )?;
                let project = self.project(project_id)?.clone();
                ensure(
                    project.status == ProjectStatus::Accepted,
                    "project_not_accepted",
                )?;
                let acceptance = self
                    .acceptances
                    .get(
                        project
                            .acceptance_id
                            .as_deref()
                            .ok_or("project_acceptance_required")?,
                    )
                    .ok_or("acceptance_not_found")?
                    .clone();
                let delivery = self
                    .deliveries
                    .get(delivery_id)
                    .ok_or("delivery_not_found")?
                    .clone();
                let author_run = self
                    .runs
                    .values()
                    .find(|run| run.run_id == Some(acceptance.author_run_id))
                    .ok_or("closing_runtime_run_missing")?;
                ensure(
                    runtime_receipt_matches(author_run, &acceptance.evidence_refs)
                        && runtime_receipt_matches(author_run, evidence_refs),
                    "closing_runtime_receipt_required",
                )?;
                ensure(
                    delivery.project_id == *project_id
                        && delivery.acceptance_id == acceptance.acceptance_id
                        && delivery.status == DeliveryStatus::Confirmed
                        && delivery.handoff_receipt_ref.is_some(),
                    "closing_confirmed_delivery_required",
                )?;
                let review = self
                    .reviews
                    .values()
                    .find(|r| r.acceptance_id == acceptance.acceptance_id)
                    .ok_or("closing_review_required")?;
                ensure(
                    a.session_id != review.reviewer_session_id
                        && a.session_id != acceptance.author_session_id,
                    "closing_role_separation_required",
                )?;
                ensure(
                    self.incidents
                        .values()
                        .filter(|i| i.project_id.as_deref() == Some(project_id))
                        .all(|i| {
                            matches!(i.status, IncidentStatus::Resolved | IncidentStatus::Closed)
                        }),
                    "closing_incident_unresolved",
                )?;
                let receipt = CompanyClosingReceipt {
                    schema: "kiana.company-closing-receipt.v1".to_owned(),
                    receipt_id: receipt_id.clone(),
                    project_id: project_id.clone(),
                    acceptance_id: acceptance.acceptance_id.clone(),
                    delivery_id: delivery_id.clone(),
                    review_id: review.review_id.clone(),
                    author_run_id: acceptance.author_run_id,
                    author_session_id: acceptance.author_session_id.clone(),
                    reviewer_session_id: review.reviewer_session_id.clone(),
                    closer_session_id: a.session_id.clone(),
                    artifact_refs: delivery.artifact_refs.clone(),
                    evidence_refs: evidence_refs.clone(),
                    closed_at: a.now_ms,
                    waiver_ref: None,
                };
                self.closing_receipts.insert(receipt_id.clone(), receipt);
                self.set_project(project_id, ProjectStatus::Closed)?;
                self.projects
                    .get_mut(project_id)
                    .unwrap()
                    .closing_receipt_id = Some(receipt_id.clone());
                let ac = self.acceptances.get_mut(&acceptance.acceptance_id).unwrap();
                ac.status = ac.status.transition(AcceptanceStatus::Closed)?;
                ac.version += 1;
                for m in self
                    .milestones
                    .values_mut()
                    .filter(|m| m.project_id == *project_id)
                {
                    m.status = m.status.transition(MilestoneStatus::Closed)?;
                    m.version += 1;
                }
            }
            CompanyCommand::RecordOutcome {
                outcome_id,
                objective_id,
                project_id,
                observation,
            } => {
                required(outcome_id)?;
                Self::evidence(p, &observation.evidence_refs)?;
                let project = self.project(project_id)?;
                ensure(
                    project.status == ProjectStatus::Closed
                        && project.objective_refs.contains(objective_id),
                    "outcome_closed_project_required",
                )?;
                let objective = self
                    .objectives
                    .get(objective_id)
                    .ok_or("objective_not_found")?
                    .clone();
                ensure(
                    a.actor_id == objective.owner_principal_id
                        && a.now_ms >= objective.period_end
                        && observation.observed_at >= objective.period_start
                        && observation.observed_at <= objective.period_end
                        && observation.value.is_finite(),
                    "outcome_measurement_window_or_owner_invalid",
                )?;
                ensure(
                    !self.outcomes.contains_key(outcome_id),
                    "outcome_already_recorded",
                )?;
                let improved = match objective.direction {
                    MetricDirection::AtLeast => observation.value > objective.baseline,
                    MetricDirection::AtMost => observation.value < objective.baseline,
                };
                let status = if objective.target_met(observation.value) {
                    OutcomeStatus::Realized
                } else if improved {
                    OutcomeStatus::PartiallyRealized
                } else {
                    OutcomeStatus::NotRealized
                };
                self.outcomes.insert(
                    outcome_id.clone(),
                    Outcome {
                        outcome_id: outcome_id.clone(),
                        objective_id: objective_id.clone(),
                        project_id: project_id.clone(),
                        metric_observations: vec![observation.clone()],
                        target_snapshot: objective.clone(),
                        measurement_start: objective.period_start,
                        measurement_end: objective.period_end,
                        owner_id: a.actor_id.clone(),
                        review_at: a.now_ms,
                        evidence_refs: observation.evidence_refs.clone(),
                        status,
                    },
                );
            }
            CompanyCommand::RequestChange { change } => {
                change.validate()?;
                let project = self.project(&change.project_id)?;
                ensure(
                    change.requested_by == a.actor_id
                        && change.version == 1
                        && change.status == ChangeStatus::Draft
                        && change.decision.is_none()
                        && change.proposed_baseline_version > project.version.saturating_add(2),
                    "change_initial_state_invalid",
                )?;
                ensure(
                    !self.changes.contains_key(&change.change_id),
                    "change_already_exists",
                )?;
                self.set_project(&change.project_id, ProjectStatus::ChangePending)?;
                let mut proposed = change.clone();
                proposed.status = ChangeStatus::PendingDecision;
                self.changes.insert(change.change_id.clone(), proposed);
            }
            CompanyCommand::DecideChange {
                change_id,
                approve,
                decision_ref,
            } => {
                Self::evidence(p, &[decision_ref.clone()])?;
                let change = self
                    .changes
                    .get(change_id)
                    .ok_or("change_not_found")?
                    .clone();
                ensure(
                    change.status == ChangeStatus::PendingDecision,
                    "change_not_pending",
                )?;
                ensure(
                    !self
                        .runs
                        .values()
                        .any(|r| r.project_id == change.project_id && !r.status.is_terminal()),
                    "change_active_run_denied",
                )?;
                self.set_project(&change.project_id, ProjectStatus::Active)?;
                if *approve {
                    let project = self.projects.get_mut(&change.project_id).unwrap();
                    ensure(
                        change.proposed_baseline_version > project.version,
                        "change_baseline_version_stale",
                    )?;
                    project.scope_baseline = change.proposed_scope_baseline.clone();
                    project.success_criteria = change.proposed_success_criteria.clone();
                    project.version = change.proposed_baseline_version;
                }
                let change = self.changes.get_mut(change_id).unwrap();
                change.status = change.status.transition(if *approve {
                    ChangeStatus::Approved
                } else {
                    ChangeStatus::Rejected
                })?;
                change.decision = Some(decision_ref.clone());
                change.version += 1;
            }
            CompanyCommand::IdentifyRisk { risk } => {
                risk.validate()?;
                self.project(&risk.project_id)?;
                ensure(
                    risk.status == RiskStatus::Identified
                        && risk.incident_id.is_none()
                        && !self.risks.contains_key(&risk.risk_id),
                    "risk_initial_state_invalid",
                )?;
                self.risks.insert(risk.risk_id.clone(), risk.clone());
            }
            CompanyCommand::AdvanceRisk {
                risk_id,
                status,
                incident_id,
            } => {
                let risk = self.risks.get(risk_id).ok_or("risk_not_found")?;
                ensure(
                    risk.owner_id == a.actor_id || a.role_id == "sponsor",
                    "risk_owner_mismatch",
                )?;
                if *status == RiskStatus::Materialized {
                    let incident = self
                        .incidents
                        .get(incident_id.as_deref().ok_or("risk_incident_required")?)
                        .ok_or("incident_not_found")?;
                    ensure(
                        incident.risk_id.as_deref() == Some(risk_id)
                            && incident.project_id.as_deref() == Some(&risk.project_id),
                        "risk_incident_mismatch",
                    )?;
                }
                if *status == RiskStatus::Closed && risk.status == RiskStatus::Materialized {
                    ensure(
                        self.incidents
                            .get(risk.incident_id.as_deref().unwrap_or_default())
                            .is_some_and(|i| {
                                matches!(
                                    i.status,
                                    IncidentStatus::Resolved | IncidentStatus::Closed
                                )
                            }),
                        "risk_incident_unresolved",
                    )?;
                }
                let risk = self.risks.get_mut(risk_id).unwrap();
                risk.status = risk.status.transition(*status)?;
                if incident_id.is_some() {
                    risk.incident_id = incident_id.clone();
                }
            }
            CompanyCommand::OpenIncident { incident } => self.insert_incident(incident, p)?,
            CompanyCommand::AdvanceIncident {
                incident_id,
                status,
                evidence_refs,
            } => {
                Self::evidence(p, evidence_refs)?;
                let incident = self
                    .incidents
                    .get_mut(incident_id)
                    .ok_or("incident_not_found")?;
                ensure(
                    incident.owner_id == a.actor_id || a.role_id == "sponsor",
                    "incident_owner_mismatch",
                )?;
                incident.status = incident.status.transition(*status)?;
                incident.evidence_refs.extend(evidence_refs.clone());
                incident.evidence_refs.sort();
                incident.evidence_refs.dedup();
                incident.timeline.push(format!("{:?}@{}", status, a.now_ms));
            }
            CompanyCommand::PauseProject { project_id } => {
                self.set_project(project_id, ProjectStatus::Paused)?
            }
            CompanyCommand::ResumeProject { project_id } => {
                self.set_project(project_id, ProjectStatus::Active)?
            }
            CompanyCommand::RequestCancelProject { project_id, reason } => {
                required(reason)?;
                self.set_project(project_id, ProjectStatus::CancelRequested)?;
            }
            CompanyCommand::ConfirmCancelProject {
                project_id,
                incident,
            } => {
                if let Some(incident) = incident {
                    ensure(
                        incident.project_id.as_deref() == Some(project_id),
                        "project_incident_mismatch",
                    )?;
                    self.insert_incident(incident, p)?;
                    self.set_project(project_id, ProjectStatus::ResultUnknown)?;
                    self.projects.get_mut(project_id).unwrap().incident_id =
                        Some(incident.incident_id.clone());
                } else {
                    ensure(p.project_runs_stopped, "project_cancel_stop_unconfirmed")?;
                    self.set_project(project_id, ProjectStatus::Cancelled)?;
                }
            }
            CompanyCommand::FailProject {
                project_id,
                reason,
                evidence_refs,
            } => {
                required(reason)?;
                Self::evidence(p, evidence_refs)?;
                ensure(p.project_runs_stopped, "project_failure_runs_unresolved")?;
                self.set_project(project_id, ProjectStatus::Failed)?;
            }
            CompanyCommand::ArchiveProject { project_id } => {
                self.set_project(project_id, ProjectStatus::Archived)?
            }
        }
        Ok(())
    }
}

impl Acceptance {
    pub fn validate(&self) -> CompanyResult<()> {
        for value in [
            &self.acceptance_id,
            &self.project_id,
            &self.milestone_id,
            &self.work_packet_id,
        ] {
            required(value)?;
        }
        self.criteria_snapshot.validate()?;
        list(&self.criteria_snapshot.criteria())?;
        list(&self.evidence_refs)?;
        ensure(
            !self.author_session_id.is_empty()
                && self.version > 0
                && !self.author_run_id.as_uuid().is_nil()
                && ((self.decision.is_none()
                    && self.decided_at.is_none()
                    && self.decision_maker_id.is_none())
                    || (self.decision.is_some()
                        && self.decided_at.is_some_and(|value| value > 0)
                        && self
                            .decision_maker_id
                            .as_deref()
                            .is_some_and(|value| !value.trim().is_empty()))),
            "acceptance_identity_required",
        )
    }
}
impl CompanyReview {
    pub fn validate(&self) -> CompanyResult<()> {
        for value in [&self.review_id, &self.acceptance_id, &self.reviewer_id] {
            required(value)?;
        }
        ensure(
            !self.reviewer_session_id.is_empty() && self.recorded_at > 0,
            "review_identity_required",
        )?;
        self.criteria_snapshot.validate()?;
        ensure(
            self.criterion_results.keys().cloned().collect::<Vec<_>>()
                == self.criteria_snapshot.criteria(),
            "review_criteria_snapshot_mismatch",
        )?;
        list(&self.evidence_refs)
    }
}
impl MetricObservation {
    pub fn validate(&self) -> CompanyResult<()> {
        ensure(
            self.value.is_finite() && self.observed_at > 0,
            "metric_observation_invalid",
        )?;
        list(&self.evidence_refs)
    }
}
impl Outcome {
    pub fn validate(&self) -> CompanyResult<()> {
        for value in [
            &self.outcome_id,
            &self.objective_id,
            &self.project_id,
            &self.owner_id,
        ] {
            required(value)?;
        }
        self.target_snapshot.validate()?;
        list(&self.evidence_refs)?;
        ensure(
            self.measurement_start < self.measurement_end
                && !self.metric_observations.is_empty()
                && self.metric_observations.iter().all(|o| {
                    o.value.is_finite()
                        && o.observed_at >= self.measurement_start
                        && o.observed_at <= self.measurement_end
                }),
            "outcome_measurement_invalid",
        )
    }
}
