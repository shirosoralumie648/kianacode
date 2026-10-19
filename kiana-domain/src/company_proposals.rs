//! Structured Company proposal and atomic Plan approval facts.
//!
//! A proposal can only enter the ledger from an explicit role-task result. The intake contract
//! names the surface it came from, and every text surface (README, stdout, assistant message,
//! transcript) is refused before the body is looked at: this module contains no command-text
//! interpreter, so command JSON inside prose can never become an effect.
//!
//! Approval is one compare-and-swap transaction over a fully validated preview. The complete
//! Plan/Milestone/Packet reference set and the follow-on handoff intents are built in local
//! values before anything is inserted, so a failure on any later packet leaves no half-approved
//! plan. The transaction freezes the reviewed preview digest, the exact plan graph and its
//! dependency versions. It grants no capability, path, budget or runtime lease.
use crate::{
    json_digest, CriterionId, FrozenPlanGraph, PlanEdgeKind, PlanNodeKind, PlanProposal,
    ResultContract, ROLE_ARCHITECT, ROLE_PM,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const COMPANY_PLAN_PROPOSAL_SCHEMA: &str = "kiana.company-plan-proposal.v1";
pub const COMPANY_PLAN_APPROVAL_SCHEMA: &str = "kiana.company-plan-approval.v1";
/// One explicit structured result returned by an authorized role task.
pub const COMPANY_ROLE_TASK_RESULT_SCHEMA: &str = "kiana.company-role-task-result.v1";
/// The only envelope the proposal ledger accepts.
pub const COMPANY_PLAN_PROPOSAL_INTAKE_SCHEMA: &str = "kiana.company-plan-proposal-intake.v1";
/// Inert, fully validated Plan preview of one proposal.
pub const COMPANY_PLAN_PREVIEW_SCHEMA: &str = "kiana.company-plan-preview.v1";
/// Frozen reference set committed by one approval CAS.
pub const COMPANY_PLAN_MATERIALIZATION_SCHEMA: &str = "kiana.company-plan-materialization.v1";

fn required(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("company_proposal_field_required_or_too_large")
    } else {
        Ok(())
    }
}

fn digest_ref(value: &str, reason: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(reason);
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(reason);
    }
    Ok(())
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

/// Surface that produced a candidate proposal. Only `RoleTaskResult` carries an explicit
/// structured result; every other surface is prose and is refused by the intake contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyProposalSourceKind {
    RoleTaskResult,
    Readme,
    Stdout,
    AssistantMessage,
    Transcript,
}

/// One explicit structured result returned by an authorized planning-role task.
///
/// The result is a typed value. It is never reconstructed from text, and the ledger never reads
/// `observed_text`; the only authority is this contract's own proposal payload and digest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyRoleTaskResult {
    pub schema: String,
    pub task_id: String,
    pub assignment_id: String,
    pub role_id: String,
    pub session_id: String,
    pub result_schema: String,
    pub proposal: CompanyPlanProposal,
    pub digest: String,
}

impl CompanyRoleTaskResult {
    pub fn new(
        task_id: impl Into<String>,
        assignment_id: impl Into<String>,
        role_id: impl Into<String>,
        session_id: impl Into<String>,
        proposal: CompanyPlanProposal,
    ) -> Result<Self, &'static str> {
        let mut result = Self {
            schema: COMPANY_ROLE_TASK_RESULT_SCHEMA.to_owned(),
            task_id: task_id.into(),
            assignment_id: assignment_id.into(),
            role_id: role_id.into(),
            session_id: session_id.into(),
            result_schema: proposal.result.output_schema.clone(),
            proposal,
            digest: String::new(),
        };
        result.digest = result.canonical_digest();
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_ROLE_TASK_RESULT_SCHEMA {
            return Err("company_role_task_result_schema_invalid");
        }
        for value in [
            &self.task_id,
            &self.assignment_id,
            &self.role_id,
            &self.session_id,
            &self.result_schema,
        ] {
            required(value)?;
        }
        if !matches!(self.role_id.as_str(), ROLE_PM | ROLE_ARCHITECT) {
            return Err("company_proposal_source_role_denied");
        }
        self.proposal.validate()?;
        if self.assignment_id != self.proposal.source_assignment
            || self.session_id != self.proposal.source_session
        {
            return Err("company_proposal_source_binding_mismatch");
        }
        if self.result_schema != self.proposal.result.output_schema {
            return Err("company_role_task_output_schema_mismatch");
        }
        if self.digest != self.canonical_digest() {
            return Err("company_role_task_result_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "task_id": self.task_id,
            "assignment_id": self.assignment_id,
            "role_id": self.role_id,
            "session_id": self.session_id,
            "result_schema": self.result_schema,
            "proposal_digest": self.proposal.digest,
            "proposal": self.proposal,
        }))
    }
}

/// The only envelope that may enter [`CompanyProposalLedger`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanProposalIntake {
    pub schema: String,
    pub source_kind: CompanyProposalSourceKind,
    /// Raw surface text retained for audit only. It is deliberately never parsed.
    #[serde(default)]
    pub observed_text: Option<String>,
    pub task: Option<CompanyRoleTaskResult>,
}

impl CompanyPlanProposalIntake {
    pub fn role_task(task: CompanyRoleTaskResult) -> Self {
        Self {
            schema: COMPANY_PLAN_PROPOSAL_INTAKE_SCHEMA.to_owned(),
            source_kind: CompanyProposalSourceKind::RoleTaskResult,
            observed_text: None,
            task: Some(task),
        }
    }

    /// A prose surface (README, stdout, assistant message, transcript). The body is audit data;
    /// it can never be promoted into a task result.
    pub fn text_surface(kind: CompanyProposalSourceKind, observed_text: impl Into<String>) -> Self {
        Self {
            schema: COMPANY_PLAN_PROPOSAL_INTAKE_SCHEMA.to_owned(),
            source_kind: kind,
            observed_text: Some(observed_text.into()),
            task: None,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PLAN_PROPOSAL_INTAKE_SCHEMA {
            return Err("company_proposal_intake_schema_invalid");
        }
        if self.source_kind != CompanyProposalSourceKind::RoleTaskResult {
            return Err("company_proposal_source_not_role_task_result");
        }
        let task = self
            .task
            .as_ref()
            .ok_or("company_proposal_source_not_role_task_result")?;
        task.validate()?;
        Ok(())
    }
}

/// Server-derived inputs the preview is validated against. The values are supplied by the
/// approval boundary (frozen Charter baseline, coverage digest, expected node dependency
/// versions and the project budget bound), never by the proposal author.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanMaterializationBasis {
    pub project_id: String,
    /// Company revision the caller read before previewing. One approval CAS may consume it.
    pub expected_revision: u64,
    pub charter_baseline_version: u64,
    pub coverage_digest: String,
    #[serde(default)]
    pub required_criteria: Vec<CriterionId>,
    /// Exact dependency version of every plan node.
    pub node_versions: BTreeMap<String, u64>,
    pub max_nodes: u64,
    pub max_packets: u64,
}

impl CompanyPlanMaterializationBasis {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.project_id.trim().is_empty()
            || self.expected_revision == u64::MAX
            || self.charter_baseline_version == 0
            || self.max_nodes == 0
            || self.max_packets == 0
        {
            return Err("company_plan_basis_invalid");
        }
        digest_ref(
            &self.coverage_digest,
            "company_plan_basis_coverage_digest_invalid",
        )?;
        if self.node_versions.is_empty()
            || self.node_versions.len() as u64 > self.max_nodes
            || self
                .node_versions
                .iter()
                .any(|(id, version)| required(id).is_err() || *version == 0)
        {
            return Err("company_plan_basis_node_versions_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanMilestoneRef {
    pub milestone_id: String,
    pub version: u64,
    pub required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanPacketRef {
    pub packet_id: String,
    pub version: u64,
    pub required: bool,
    pub milestone_id: String,
    /// Blocking packet dependencies declared by non-parent-child, non-related edges.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Inert follow-on handoff intent. It names no lease, grant or run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanHandoffIntent {
    pub plan_digest: String,
    pub packet_id: String,
    pub milestone_id: String,
    pub from_department: String,
    pub to_department: String,
    pub target_role: String,
    #[serde(default)]
    pub runtime_authority: bool,
}

/// Fully validated Plan preview awaiting approval. It is a value, not a committed fact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanPreview {
    pub schema: String,
    pub proposal_id: String,
    pub proposal_digest: String,
    pub project_id: String,
    pub plan_version: u64,
    pub plan_digest: String,
    pub charter_baseline_version: u64,
    pub coverage_digest: String,
    pub expected_revision: u64,
    #[serde(default)]
    pub required_criteria: Vec<CriterionId>,
    pub frozen_plan: FrozenPlanGraph,
    #[serde(default)]
    pub milestones: Vec<CompanyPlanMilestoneRef>,
    #[serde(default)]
    pub packets: Vec<CompanyPlanPacketRef>,
    #[serde(default)]
    pub handoffs: Vec<CompanyPlanHandoffIntent>,
    pub digest: String,
}

impl CompanyPlanPreview {
    /// Validate the whole candidate graph once and derive the complete reference set. Every check
    /// runs against local values; the caller commits the preview only after this returns `Ok`.
    fn build(
        proposal: &CompanyPlanProposal,
        basis: &CompanyPlanMaterializationBasis,
    ) -> Result<Self, &'static str> {
        proposal.validate()?;
        basis.validate()?;
        let plan = &proposal.plan;
        if basis.project_id != plan.project_id {
            return Err("company_plan_preview_project_mismatch");
        }
        if basis.charter_baseline_version != plan.charter_baseline_version {
            return Err("company_plan_preview_baseline_mismatch");
        }
        if basis.coverage_digest != plan.coverage_digest {
            return Err("company_plan_preview_coverage_mismatch");
        }
        let mut required_criteria = plan.required_criteria.clone();
        required_criteria.sort();
        let mut expected_criteria = basis.required_criteria.clone();
        expected_criteria.sort();
        if required_criteria != expected_criteria {
            return Err("company_plan_preview_criteria_mismatch");
        }
        if plan.nodes.len() as u64 > basis.max_nodes {
            return Err("company_plan_preview_node_budget_exceeded");
        }
        let packet_count = plan
            .nodes
            .values()
            .filter(|node| node.kind == PlanNodeKind::Packet)
            .count() as u64;
        if packet_count > basis.max_packets {
            return Err("company_plan_preview_packet_budget_exceeded");
        }
        // Input versions are an exact closed set: an unknown, missing or drifted node version is
        // refused before any reference is derived.
        if basis.node_versions.len() != plan.nodes.len() {
            return Err("company_plan_preview_node_version_missing");
        }
        for (id, node) in &plan.nodes {
            let Some(version) = basis.node_versions.get(id) else {
                return Err("company_plan_preview_node_version_missing");
            };
            if *version != node.version {
                return Err("company_plan_preview_node_version_mismatch");
            }
        }

        let frozen_plan = FrozenPlanGraph::from_plan(plan)?;
        let milestones = plan
            .nodes
            .iter()
            .filter(|(_, node)| node.kind == PlanNodeKind::Milestone)
            .map(|(id, node)| CompanyPlanMilestoneRef {
                milestone_id: id.clone(),
                version: node.version,
                required: node.required,
            })
            .collect::<Vec<_>>();

        let mut packets = Vec::with_capacity(packet_count as usize);
        let mut handoffs = Vec::with_capacity(packet_count as usize);
        for (id, node) in &plan.nodes {
            if node.kind != PlanNodeKind::Packet {
                continue;
            }
            // Each packet has exactly one parent Milestone. A packet that names zero or several
            // parents is a planning hole; the failure is reported here, after the earlier nodes
            // have already validated, and the whole approval still commits nothing.
            let mut parents = plan
                .edges
                .iter()
                .filter(|edge| edge.kind == PlanEdgeKind::ParentChild && edge.to == *id)
                .map(|edge| edge.from.clone())
                .collect::<Vec<_>>();
            parents.sort();
            parents.dedup();
            let [milestone_id] = parents.as_slice() else {
                return Err("company_plan_preview_packet_parent_invalid");
            };
            let mut dependencies = plan
                .edges
                .iter()
                .filter(|edge| {
                    edge.to == *id
                        && !matches!(edge.kind, PlanEdgeKind::ParentChild | PlanEdgeKind::Related)
                })
                .map(|edge| edge.from.clone())
                .collect::<Vec<_>>();
            dependencies.sort();
            dependencies.dedup();
            packets.push(CompanyPlanPacketRef {
                packet_id: id.clone(),
                version: node.version,
                required: node.required,
                milestone_id: milestone_id.clone(),
                dependencies,
            });
            handoffs.push(CompanyPlanHandoffIntent {
                plan_digest: plan.digest.clone(),
                packet_id: id.clone(),
                milestone_id: milestone_id.clone(),
                from_department: "planning".to_owned(),
                to_department: "executing".to_owned(),
                target_role: "builder".to_owned(),
                runtime_authority: false,
            });
        }

        let mut preview = Self {
            schema: COMPANY_PLAN_PREVIEW_SCHEMA.to_owned(),
            proposal_id: proposal.proposal_id.clone(),
            proposal_digest: proposal.digest.clone(),
            project_id: plan.project_id.clone(),
            plan_version: plan.plan_version,
            plan_digest: plan.digest.clone(),
            charter_baseline_version: plan.charter_baseline_version,
            coverage_digest: plan.coverage_digest.clone(),
            expected_revision: basis.expected_revision,
            required_criteria,
            frozen_plan,
            milestones,
            packets,
            handoffs,
            digest: String::new(),
        };
        preview.digest = preview.canonical_digest();
        preview.validate()?;
        Ok(preview)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PLAN_PREVIEW_SCHEMA
            || self.expected_revision == u64::MAX
            || self.plan_version == 0
            || self.charter_baseline_version == 0
        {
            return Err("company_plan_preview_invalid");
        }
        for value in [
            &self.proposal_id,
            &self.proposal_digest,
            &self.project_id,
            &self.plan_digest,
            &self.coverage_digest,
        ] {
            required(value)?;
        }
        digest_ref(
            &self.proposal_digest,
            "company_plan_preview_proposal_digest_invalid",
        )?;
        digest_ref(
            &self.plan_digest,
            "company_plan_preview_plan_digest_invalid",
        )?;
        self.frozen_plan.validate()?;
        if self.frozen_plan.project_id != self.project_id
            || self.frozen_plan.plan_digest != self.plan_digest
            || self.frozen_plan.plan_version != self.plan_version
            || self.frozen_plan.charter_baseline_version != self.charter_baseline_version
            || self.frozen_plan.coverage_digest != self.coverage_digest
        {
            return Err("company_plan_preview_frozen_plan_mismatch");
        }
        if self.milestones.is_empty() || self.packets.is_empty() {
            return Err("company_plan_preview_reference_set_invalid");
        }
        for milestone in &self.milestones {
            required(&milestone.milestone_id)?;
            if milestone.version == 0
                || self.frozen_plan.node_versions.get(&milestone.milestone_id)
                    != Some(&milestone.version)
            {
                return Err("company_plan_preview_milestone_reference_invalid");
            }
        }
        for packet in &self.packets {
            required(&packet.packet_id)?;
            if packet.version == 0
                || self.frozen_plan.node_versions.get(&packet.packet_id) != Some(&packet.version)
                || !self
                    .milestones
                    .iter()
                    .any(|milestone| milestone.milestone_id == packet.milestone_id)
            {
                return Err("company_plan_preview_packet_reference_invalid");
            }
        }
        if self.handoffs.len() != self.packets.len() {
            return Err("company_plan_preview_handoff_set_invalid");
        }
        for handoff in &self.handoffs {
            if handoff.runtime_authority
                || handoff.plan_digest != self.plan_digest
                || !self
                    .packets
                    .iter()
                    .any(|packet| packet.packet_id == handoff.packet_id)
            {
                return Err("company_plan_preview_handoff_set_invalid");
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("company_plan_preview_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "proposal_id": self.proposal_id,
            "proposal_digest": self.proposal_digest,
            "project_id": self.project_id,
            "plan_version": self.plan_version,
            "plan_digest": self.plan_digest,
            "charter_baseline_version": self.charter_baseline_version,
            "coverage_digest": self.coverage_digest,
            "expected_revision": self.expected_revision,
            "required_criteria": self.required_criteria,
            "frozen_plan": self.frozen_plan,
            "milestones": self.milestones,
            "packets": self.packets,
            "handoffs": self.handoffs,
        }))
    }

    /// Re-verify every frozen reference and build the materialization in one local value. Any
    /// failure aborts before the caller mutates the ledger.
    fn materialize(
        &self,
        approval: &CompanyPlanApproval,
        company_revision: u64,
    ) -> Result<CompanyPlanMaterialization, &'static str> {
        self.validate()?;
        if approval.preview_digest != self.digest
            || approval.proposal_digest != self.proposal_digest
        {
            return Err("company_plan_approval_preview_mismatch");
        }
        for milestone in &self.milestones {
            if self.frozen_plan.node_versions.get(&milestone.milestone_id)
                != Some(&milestone.version)
            {
                return Err("company_plan_materialization_node_mismatch");
            }
        }
        for packet in &self.packets {
            if self.frozen_plan.node_versions.get(&packet.packet_id) != Some(&packet.version) {
                return Err("company_plan_materialization_node_mismatch");
            }
        }
        let mut materialization = CompanyPlanMaterialization {
            schema: COMPANY_PLAN_MATERIALIZATION_SCHEMA.to_owned(),
            approval_id: approval.approval_id.clone(),
            approval_digest: approval.digest.clone(),
            proposal_id: self.proposal_id.clone(),
            project_id: self.project_id.clone(),
            approved_plan_digest: self.plan_digest.clone(),
            frozen_plan: self.frozen_plan.clone(),
            milestones: self.milestones.clone(),
            packets: self.packets.clone(),
            handoffs: self.handoffs.clone(),
            company_revision,
            digest: String::new(),
        };
        materialization.digest = materialization.canonical_digest();
        materialization.validate()?;
        Ok(materialization)
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
    /// Digest of the exact preview the user reviewed and approved.
    pub preview_digest: String,
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
            &self.preview_digest,
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
        digest_ref(
            &self.preview_digest,
            "company_plan_approval_preview_digest_invalid",
        )?;
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
            "preview_digest": self.preview_digest,
            "expected_revision": self.expected_revision,
            "decision_ref": self.decision_ref,
            "approver_assignment": self.approver_assignment,
            "approver_role": self.approver_role,
            "approved_at": self.approved_at,
        }))
    }
}

/// Reference set frozen by one approval CAS. These are references, not scheduled packets: no
/// Milestone body is invented, no packet is claimed and no run is created.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPlanMaterialization {
    pub schema: String,
    pub approval_id: String,
    pub approval_digest: String,
    pub proposal_id: String,
    pub project_id: String,
    /// The plan hash the user approved; equal to the frozen graph digest.
    pub approved_plan_digest: String,
    pub frozen_plan: FrozenPlanGraph,
    #[serde(default)]
    pub milestones: Vec<CompanyPlanMilestoneRef>,
    #[serde(default)]
    pub packets: Vec<CompanyPlanPacketRef>,
    #[serde(default)]
    pub handoffs: Vec<CompanyPlanHandoffIntent>,
    /// Company revision produced by the CAS.
    pub company_revision: u64,
    pub digest: String,
}

impl CompanyPlanMaterialization {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_PLAN_MATERIALIZATION_SCHEMA || self.company_revision == 0 {
            return Err("company_plan_materialization_invalid");
        }
        for value in [
            &self.approval_id,
            &self.approval_digest,
            &self.proposal_id,
            &self.project_id,
            &self.approved_plan_digest,
        ] {
            required(value)?;
        }
        digest_ref(
            &self.approved_plan_digest,
            "company_plan_materialization_plan_digest_invalid",
        )?;
        self.frozen_plan.validate()?;
        if self.frozen_plan.plan_digest != self.approved_plan_digest {
            return Err("company_plan_materialization_plan_mismatch");
        }
        if self.milestones.is_empty() || self.packets.is_empty() {
            return Err("company_plan_materialization_reference_set_invalid");
        }
        for packet in &self.packets {
            if self.frozen_plan.node_versions.get(&packet.packet_id) != Some(&packet.version) {
                return Err("company_plan_materialization_node_mismatch");
            }
        }
        if self.handoffs.len() != self.packets.len()
            || self.handoffs.iter().any(|handoff| {
                handoff.runtime_authority || handoff.plan_digest != self.approved_plan_digest
            })
        {
            return Err("company_plan_materialization_handoff_set_invalid");
        }
        if self.digest != self.canonical_digest() {
            return Err("company_plan_materialization_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "approval_id": self.approval_id,
            "approval_digest": self.approval_digest,
            "proposal_id": self.proposal_id,
            "project_id": self.project_id,
            "approved_plan_digest": self.approved_plan_digest,
            "frozen_plan": self.frozen_plan,
            "milestones": self.milestones,
            "packets": self.packets,
            "handoffs": self.handoffs,
            "company_revision": self.company_revision,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyProposalLedger {
    pub proposals: BTreeMap<String, CompanyPlanProposal>,
    pub previews: BTreeMap<String, CompanyPlanPreview>,
    pub approvals: BTreeMap<String, CompanyPlanApproval>,
    pub materializations: BTreeMap<String, CompanyPlanMaterialization>,
    /// Revision consumed by the next approval CAS.
    pub revision: u64,
}

impl CompanyProposalLedger {
    /// Ingest one explicit role-task result and build the full preview. Text surfaces are refused
    /// by the intake contract before the body is inspected. Both facts are inserted only after
    /// the whole preview validated, so a failure on any later packet leaves no half-approved plan.
    pub fn submit(
        &mut self,
        intake: CompanyPlanProposalIntake,
        basis: CompanyPlanMaterializationBasis,
    ) -> Result<CompanyPlanPreview, &'static str> {
        intake.validate()?;
        let task = intake
            .task
            .as_ref()
            .ok_or("company_proposal_source_not_role_task_result")?;
        let proposal = &task.proposal;
        proposal.validate()?;
        if self.proposals.contains_key(&proposal.proposal_id) {
            return Err("company_proposal_duplicate");
        }
        if self.previews.contains_key(&proposal.proposal_id) {
            return Err("company_proposal_duplicate");
        }
        let preview = CompanyPlanPreview::build(proposal, &basis)?;
        self.proposals
            .insert(proposal.proposal_id.clone(), proposal.clone());
        self.previews
            .insert(proposal.proposal_id.clone(), preview.clone());
        Ok(preview)
    }

    /// One approval CAS. The preview digest, the plan digest, the CAS revision and the whole
    /// reference set are re-verified before any field changes. Replaying the exact same approval
    /// returns the already committed materialization without duplicating packets.
    pub fn approve(
        &mut self,
        approval: CompanyPlanApproval,
    ) -> Result<CompanyPlanMaterialization, &'static str> {
        approval.validate()?;
        if let Some(existing) = self.approvals.get(&approval.proposal_id) {
            if existing.digest == approval.digest {
                return self
                    .materializations
                    .get(&approval.proposal_id)
                    .cloned()
                    .ok_or("company_plan_materialization_missing");
            }
            return Err("company_plan_approval_duplicate");
        }
        let proposal = self
            .proposals
            .get(&approval.proposal_id)
            .ok_or("company_proposal_missing")?;
        let preview = self
            .previews
            .get(&approval.proposal_id)
            .ok_or("company_plan_preview_missing")?;
        if proposal.project_id != approval.project_id || proposal.digest != approval.proposal_digest
        {
            return Err("company_plan_approval_proposal_mismatch");
        }
        if preview.proposal_digest != proposal.digest || preview.plan_digest != proposal.plan.digest
        {
            return Err("company_plan_preview_proposal_mismatch");
        }
        if preview.digest != approval.preview_digest {
            return Err("company_plan_approval_preview_mismatch");
        }
        if approval.expected_revision != preview.expected_revision
            || approval.expected_revision != self.revision
        {
            return Err("company_plan_approval_revision_conflict");
        }
        // The complete materialization is built before the first mutation.
        let materialization = preview.materialize(&approval, self.revision + 1)?;
        self.revision += 1;
        self.approvals
            .insert(approval.proposal_id.clone(), approval);
        self.materializations
            .insert(proposal.proposal_id.clone(), materialization.clone());
        Ok(materialization)
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

    pub fn materialization(&self, proposal_id: &str) -> Option<&CompanyPlanMaterialization> {
        self.materializations.get(proposal_id)
    }
}
