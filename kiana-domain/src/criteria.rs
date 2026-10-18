//! Versioned Company criterion coverage graphs.
//!
//! A coverage graph is a planning fact. It records which stable criterion at the Project level
//! is refined or covered by a Milestone/Packet criterion and which criterion has concrete
//! verification evidence. It never approves work or executes a test; ControlPlane remains the
//! authority for publishing and consuming a graph.
use crate::{json_digest, AcceptanceTarget, Criterion, CriterionId, PlanProposal};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const CRITERIA_COVERAGE_SCHEMA: &str = "kiana.criteria-coverage.v1";
/// Typed Sponsor approval of one exact coverage graph. The approval binds the graph digest, the
/// approved plan digest and the planning/review decision record that justifies semantic
/// sufficiency; it never grants a capability or starts a run.
pub const CRITERIA_COVERAGE_APPROVAL_SCHEMA: &str = "kiana.criteria-coverage-approval.v1";
/// Frozen, approved coverage graph handed to the Plan/acceptance paths.
pub const FROZEN_CRITERIA_COVERAGE_SCHEMA: &str = "kiana.frozen-criteria-coverage.v1";
const MAX_CRITERIA: usize = 1_024;
const MAX_LINKS: usize = 4_096;

fn required(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("criteria_field_required_or_too_large")
    } else {
        Ok(())
    }
}

fn approval_text(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 1_024 {
        Err("criteria_coverage_approval_field_required")
    } else {
        Ok(())
    }
}

fn approval_digest(value: &str) -> Result<(), &'static str> {
    if !value.starts_with("sha256:") || value.len() > 256 {
        Err("criteria_coverage_approval_digest_invalid")
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionRelation {
    Covers,
    Refines,
    Verifies,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionCoverageEntry {
    pub criterion: Criterion,
    pub target: AcceptanceTarget,
}

impl CriterionCoverageEntry {
    fn validate(&self, project_id: &str) -> Result<(), &'static str> {
        self.criterion
            .validate()
            .map_err(|_| "criteria_entry_invalid")?;
        match &self.target {
            AcceptanceTarget::Project(id) => {
                required(id)?;
                if id != project_id {
                    return Err("criteria_cross_project_target");
                }
            }
            AcceptanceTarget::Milestone(id) | AcceptanceTarget::Packet(id) => required(id)?,
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionCoverageLink {
    pub parent_id: CriterionId,
    pub child_id: CriterionId,
    pub relation: CriterionRelation,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

impl CriterionCoverageLink {
    fn validate(&self) -> Result<(), &'static str> {
        if self.parent_id == self.child_id {
            return Err("criteria_self_link");
        }
        let mut refs = BTreeSet::new();
        if self.evidence_refs.iter().any(|reference| {
            required(reference).is_err()
                || (!reference.starts_with("artifact:")
                    && !reference.starts_with("event:")
                    && !reference.starts_with("test:"))
                || !refs.insert(reference)
        }) {
            return Err("criteria_evidence_refs_invalid");
        }
        if self.relation == CriterionRelation::Verifies && self.evidence_refs.is_empty() {
            return Err("criteria_verification_evidence_required");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionCoverageGraph {
    pub schema: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub scope_digest: String,
    pub entries: BTreeMap<CriterionId, CriterionCoverageEntry>,
    pub links: Vec<CriterionCoverageLink>,
    /// Required criteria intentionally deferred to a later plan/milestone. This is explicit
    /// incompleteness, never an implicit pass.
    #[serde(default)]
    pub pending_required: BTreeSet<CriterionId>,
    pub digest: String,
}

impl CriterionCoverageGraph {
    pub fn new(
        project_id: impl Into<String>,
        baseline_version: u64,
        scope_digest: impl Into<String>,
        entries: Vec<CriterionCoverageEntry>,
        links: Vec<CriterionCoverageLink>,
        pending_required: BTreeSet<CriterionId>,
    ) -> Result<Self, &'static str> {
        if entries.is_empty() || entries.len() > MAX_CRITERIA {
            return Err("criteria_entries_required_or_too_large");
        }
        let mut indexed = BTreeMap::new();
        for entry in entries {
            let id = entry.criterion.criterion_id;
            if indexed.insert(id, entry).is_some() {
                return Err("criteria_duplicate_identity");
            }
        }
        let mut graph = Self {
            schema: CRITERIA_COVERAGE_SCHEMA.to_owned(),
            project_id: project_id.into(),
            baseline_version,
            scope_digest: scope_digest.into(),
            entries: indexed,
            links,
            pending_required,
            digest: String::new(),
        };
        graph.digest = graph.canonical_digest();
        graph.validate()?;
        Ok(graph)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        required(&self.project_id)?;
        required(&self.scope_digest)?;
        if self.schema != CRITERIA_COVERAGE_SCHEMA || self.baseline_version == 0 {
            return Err("criteria_graph_schema_or_baseline_invalid");
        }
        if self.entries.is_empty() || self.entries.len() > MAX_CRITERIA {
            return Err("criteria_entries_required_or_too_large");
        }
        for (id, entry) in &self.entries {
            if *id != entry.criterion.criterion_id {
                return Err("criteria_identity_key_mismatch");
            }
            entry.validate(&self.project_id)?;
            if entry.criterion.scope_digest != self.scope_digest {
                return Err("criteria_scope_drift");
            }
            if entry.criterion.source_version > self.baseline_version {
                return Err("criteria_source_version_drift");
            }
        }
        if self.links.len() > MAX_LINKS {
            return Err("criteria_links_too_large");
        }
        let mut identities = BTreeSet::new();
        for link in &self.links {
            link.validate()?;
            if !identities.insert((link.parent_id, link.child_id, link.relation)) {
                return Err("criteria_duplicate_link");
            }
            let parent = self
                .entries
                .get(&link.parent_id)
                .ok_or("criteria_link_parent_missing")?;
            let child = self
                .entries
                .get(&link.child_id)
                .ok_or("criteria_link_child_missing")?;
            if link.relation != CriterionRelation::Verifies {
                if target_rank(&parent.target) >= target_rank(&child.target) {
                    return Err("criteria_link_order_invalid");
                }
                if parent.criterion.required && !child.criterion.required {
                    return Err("criteria_required_child_not_required");
                }
            }
        }
        if has_cycle(&self.links) {
            return Err("criteria_link_cycle");
        }
        // A single generic verification statement ("tests pass") cannot cover every criterion:
        // one concrete evidence reference may verify at most one parent criterion and one evidence
        // criterion may be the verification target of at most one parent criterion.
        let mut evidence_refs: BTreeMap<&str, (CriterionId, CriterionId)> = BTreeMap::new();
        let mut evidence_targets: BTreeMap<CriterionId, CriterionId> = BTreeMap::new();
        for link in &self.links {
            if link.relation != CriterionRelation::Verifies {
                continue;
            }
            for reference in &link.evidence_refs {
                if let Some(previous) =
                    evidence_refs.insert(reference.as_str(), (link.parent_id, link.child_id))
                {
                    if previous != (link.parent_id, link.child_id) {
                        return Err("criteria_evidence_ref_reused");
                    }
                }
            }
            if let Some(previous) = evidence_targets.insert(link.child_id, link.parent_id) {
                if previous != link.parent_id {
                    return Err("criteria_evidence_reused");
                }
            }
        }
        for id in &self.pending_required {
            let entry = self
                .entries
                .get(id)
                .ok_or("criteria_pending_identity_missing")?;
            if !entry.criterion.required {
                return Err("criteria_pending_not_required");
            }
        }
        for (id, entry) in &self.entries {
            if !entry.criterion.required || matches!(entry.target, AcceptanceTarget::Packet(_)) {
                continue;
            }
            let covered = self.links.iter().any(|link| {
                link.parent_id == *id
                    && matches!(
                        link.relation,
                        CriterionRelation::Covers | CriterionRelation::Refines
                    )
            });
            if !covered && !self.pending_required.contains(id) {
                return Err("criteria_required_uncovered");
            }
            if covered && self.pending_required.contains(id) {
                return Err("criteria_pending_already_covered");
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("criteria_graph_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "scope_digest": self.scope_digest,
            "entries": self.entries,
            "links": self.links,
            "pending_required": self.pending_required,
        }))
    }

    /// Locate one criterion by stable ID and origin baseline version.
    ///
    /// Coverage relations are approved against a versioned origin; a caller that only knows the
    /// text or a stale origin version cannot silently bind a changed criterion.
    pub fn resolve_origin(
        &self,
        criterion_id: CriterionId,
        source_version: u64,
    ) -> Result<&CriterionCoverageEntry, &'static str> {
        let entry = self
            .entries
            .get(&criterion_id)
            .ok_or("criteria_identity_missing")?;
        if entry.criterion.source_version != source_version {
            return Err("criteria_origin_baseline_mismatch");
        }
        Ok(entry)
    }

    /// Stable identities in canonical order. Text equality never collapses two origins.
    pub fn criteria_ids(&self) -> Vec<CriterionId> {
        self.entries.keys().copied().collect()
    }

    pub fn uncovered_required(&self) -> Vec<CriterionId> {
        self.entries
            .iter()
            .filter_map(|(id, entry)| {
                if entry.criterion.required
                    && !self.pending_required.contains(id)
                    && !self.links.iter().any(|link| {
                        link.parent_id == *id
                            && match link.relation {
                                CriterionRelation::Covers | CriterionRelation::Refines => true,
                                CriterionRelation::Verifies => {
                                    matches!(entry.target, AcceptanceTarget::Packet(_))
                                        && link.validate().is_ok()
                                        && self.entries.contains_key(&link.child_id)
                                }
                            }
                    })
                {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn coverage_gaps(&self) -> Vec<CriterionId> {
        self.pending_required.iter().copied().collect()
    }

    pub fn trace(&self, root: CriterionId) -> Result<Vec<CriterionId>, &'static str> {
        if !self.entries.contains_key(&root) {
            return Err("criteria_trace_root_missing");
        }
        let mut trace = Vec::new();
        trace_node(root, &self.links, &mut trace);
        Ok(trace)
    }
}

/// Explicit Sponsor approval of one exact coverage graph version.
///
/// The approval names the graph digest, the approved plan digest and the planning/review decision
/// record that carries semantic sufficiency. It is a server-derived planning fact: it grants no
/// capability, budget, path or lease and cannot execute anything by itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionCoverageApproval {
    pub schema: String,
    pub approval_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub graph_digest: String,
    pub plan_digest: String,
    pub decision_ref: String,
    pub approver_assignment: String,
    pub approver_role: String,
    pub approved_at: u64,
    pub digest: String,
}

impl CriterionCoverageApproval {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        approval_id: impl Into<String>,
        project_id: impl Into<String>,
        baseline_version: u64,
        graph_digest: impl Into<String>,
        plan_digest: impl Into<String>,
        decision_ref: impl Into<String>,
        approver_assignment: impl Into<String>,
        approver_role: impl Into<String>,
        approved_at: u64,
    ) -> Result<Self, &'static str> {
        let mut approval = Self {
            schema: CRITERIA_COVERAGE_APPROVAL_SCHEMA.to_owned(),
            approval_id: approval_id.into(),
            project_id: project_id.into(),
            baseline_version,
            graph_digest: graph_digest.into(),
            plan_digest: plan_digest.into(),
            decision_ref: decision_ref.into(),
            approver_assignment: approver_assignment.into(),
            approver_role: approver_role.into(),
            approved_at,
            digest: String::new(),
        };
        approval.digest = approval.canonical_digest();
        approval.validate()?;
        Ok(approval)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CRITERIA_COVERAGE_APPROVAL_SCHEMA
            || self.baseline_version == 0
            || self.approved_at == 0
        {
            return Err("criteria_coverage_approval_invalid");
        }
        approval_text(&self.approval_id)?;
        approval_text(&self.project_id)?;
        approval_text(&self.approver_assignment)?;
        approval_digest(&self.graph_digest)?;
        approval_digest(&self.plan_digest)?;
        // Only the Sponsor approves the exact graph; a planning role can propose it but not sign it.
        if self.approver_role != "sponsor" {
            return Err("criteria_coverage_approval_sponsor_required");
        }
        // Semantic sufficiency needs an independent planning/review record, not an LLM summary.
        if !self.decision_ref.starts_with("artifact:") && !self.decision_ref.starts_with("event:") {
            return Err("criteria_coverage_approval_decision_ref_invalid");
        }
        if self.digest != self.canonical_digest() {
            return Err("criteria_coverage_approval_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "approval_id": self.approval_id,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "graph_digest": self.graph_digest,
            "plan_digest": self.plan_digest,
            "decision_ref": self.decision_ref,
            "approver_assignment": self.approver_assignment,
            "approver_role": self.approver_role,
            "approved_at": self.approved_at,
        }))
    }
}

/// Approved coverage graph frozen into a Project baseline.
///
/// Replaces text-set inference: consumers read typed criterion identities from the frozen graph,
/// so two criteria with identical text but different origins never collapse into one check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenCriterionCoverage {
    pub schema: String,
    pub graph: CriterionCoverageGraph,
    pub approval: CriterionCoverageApproval,
    pub digest: String,
}

impl FrozenCriterionCoverage {
    pub fn freeze(
        graph: CriterionCoverageGraph,
        approval: CriterionCoverageApproval,
    ) -> Result<Self, &'static str> {
        let mut frozen = Self {
            schema: FROZEN_CRITERIA_COVERAGE_SCHEMA.to_owned(),
            graph,
            approval,
            digest: String::new(),
        };
        frozen.digest = frozen.canonical_digest();
        frozen.validate()?;
        Ok(frozen)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != FROZEN_CRITERIA_COVERAGE_SCHEMA {
            return Err("criteria_coverage_freeze_schema_invalid");
        }
        self.approval.validate()?;
        self.graph.validate()?;
        if self.approval.project_id != self.graph.project_id {
            return Err("criteria_coverage_project_mismatch");
        }
        if self.approval.baseline_version != self.graph.baseline_version {
            return Err("criteria_coverage_baseline_mismatch");
        }
        if self.approval.graph_digest != self.graph.digest
            || self.graph.digest != self.graph.canonical_digest()
        {
            return Err("criteria_coverage_graph_digest_mismatch");
        }
        if self.digest != self.canonical_digest() {
            return Err("criteria_coverage_freeze_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "graph": self.graph,
            "approval": self.approval,
        }))
    }

    /// Machine-verify the typed Plan coverage binding.
    ///
    /// The plan must carry this exact coverage digest and origin baseline, must be the digest the
    /// approval signed, and every node criterion reference must resolve to a frozen identity. A
    /// required Milestone criterion that is neither covered nor explicitly pending and that no
    /// plan node is responsible for fails the completeness check.
    pub fn bind_plan(&self, plan: &PlanProposal) -> Result<(), &'static str> {
        self.validate()?;
        if plan.project_id != self.graph.project_id {
            return Err("plan_coverage_project_mismatch");
        }
        if plan.charter_baseline_version != self.graph.baseline_version {
            return Err("plan_coverage_baseline_mismatch");
        }
        if plan.coverage_digest != self.graph.digest {
            return Err("plan_coverage_digest_mismatch");
        }
        if plan.digest != self.approval.plan_digest {
            return Err("plan_coverage_plan_digest_mismatch");
        }
        for node in plan.nodes.values() {
            for reference in &node.criterion_refs {
                if !self.graph.entries.contains_key(reference) {
                    return Err("plan_coverage_reference_unknown");
                }
            }
        }
        for (id, entry) in &self.graph.entries {
            if entry.criterion.required
                && !self.graph.pending_required.contains(id)
                && matches!(entry.target, AcceptanceTarget::Milestone(_))
                && !plan
                    .nodes
                    .values()
                    .any(|node| node.criterion_refs.contains(id))
            {
                return Err("plan_coverage_required_criterion_unassigned");
            }
        }
        Ok(())
    }

    /// Typed criteria in canonical identity order; ready for a frozen acceptance snapshot.
    pub fn frozen_criteria(&self) -> Vec<Criterion> {
        let mut criteria = self
            .graph
            .entries
            .values()
            .map(|entry| entry.criterion.clone())
            .collect::<Vec<_>>();
        criteria.sort_by_key(|criterion| criterion.criterion_id);
        criteria
    }

    pub fn criteria_for(&self, target: &AcceptanceTarget) -> Vec<Criterion> {
        self.graph
            .entries
            .values()
            .filter(|entry| &entry.target == target)
            .map(|entry| entry.criterion.clone())
            .collect()
    }

    pub fn criteria_ids(&self) -> Vec<CriterionId> {
        self.graph.criteria_ids()
    }

    pub fn uncovered_required(&self) -> Vec<CriterionId> {
        self.graph.uncovered_required()
    }

    /// Required criteria intentionally deferred. Explicit incompleteness, never an implicit pass.
    pub fn coverage_gaps(&self) -> Vec<CriterionId> {
        self.graph.coverage_gaps()
    }

    pub fn trace(&self, root: CriterionId) -> Result<Vec<CriterionId>, &'static str> {
        self.graph.trace(root)
    }
}

fn has_cycle(links: &[CriterionCoverageLink]) -> bool {
    let mut edges: BTreeMap<CriterionId, Vec<CriterionId>> = BTreeMap::new();
    for link in links.iter().filter(|link| {
        matches!(
            link.relation,
            CriterionRelation::Covers | CriterionRelation::Refines
        )
    }) {
        edges.entry(link.parent_id).or_default().push(link.child_id);
    }
    let mut state = BTreeMap::new();
    for root in edges.keys().copied() {
        if visit_cycle(root, &edges, &mut state) {
            return true;
        }
    }
    false
}

fn visit_cycle(
    node: CriterionId,
    edges: &BTreeMap<CriterionId, Vec<CriterionId>>,
    state: &mut BTreeMap<CriterionId, u8>,
) -> bool {
    match state.get(&node).copied() {
        Some(1) => return true,
        Some(2) => return false,
        _ => {}
    }
    state.insert(node, 1);
    if let Some(children) = edges.get(&node) {
        for child in children {
            if visit_cycle(*child, edges, state) {
                return true;
            }
        }
    }
    state.insert(node, 2);
    false
}

fn trace_node(node: CriterionId, links: &[CriterionCoverageLink], trace: &mut Vec<CriterionId>) {
    if trace.contains(&node) {
        return;
    }
    trace.push(node);
    let mut children = links
        .iter()
        .filter(|link| link.parent_id == node)
        .map(|link| link.child_id)
        .collect::<Vec<_>>();
    children.sort();
    children.dedup();
    for child in children {
        trace_node(child, links, trace);
    }
}

fn target_rank(target: &AcceptanceTarget) -> u8 {
    match target {
        AcceptanceTarget::Project(_) => 0,
        AcceptanceTarget::Milestone(_) => 1,
        AcceptanceTarget::Packet(_) => 2,
    }
}
