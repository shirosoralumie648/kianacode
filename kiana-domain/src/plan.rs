//! Versioned Company Plan/WorkGraph proposal contracts.
//!
//! Plans are validated as data before a Company command publishes them. This module only
//! computes deterministic graph facts; it never schedules a packet, claims a lease or executes a
//! capability.
use crate::{json_digest, CriterionId};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const PLAN_PROPOSAL_SCHEMA: &str = "kiana.company-plan-proposal.v1";
/// Open batch accumulator for a candidate plan graph. Nodes, blocking edges, required criteria
/// and the read scope may be pushed in any order; the complete graph is validated exactly once
/// when the draft is frozen.
pub const PLAN_DRAFT_SCHEMA: &str = "kiana.company-plan-draft.v1";
/// Frozen dependency versions of one admitted plan revision.
pub const FROZEN_PLAN_GRAPH_SCHEMA: &str = "kiana.frozen-plan-graph.v1";
/// Deterministic batch validation report for a candidate graph.
pub const PLAN_GRAPH_REPORT_SCHEMA: &str = "kiana.company-plan-graph-report.v1";
const MAX_NODES: usize = 1_024;
const MAX_EDGES: usize = 4_096;
const MAX_REQUIRED_CRITERIA: usize = 1_024;
const MAX_READ_SCOPE: usize = 1_024;

fn required(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("plan_field_required_or_too_large")
    } else {
        Ok(())
    }
}

fn digest(value: &str, reason: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(reason);
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(reason);
    }
    Ok(())
}

/// Reading-scope entries are typed references; a plan cannot declare a free-text reading range.
fn read_reference(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty()
        || value.len() > 4_096
        || (!value.starts_with("artifact:")
            && !value.starts_with("event:")
            && !value.starts_with("test:"))
    {
        return Err("plan_read_scope_reference_invalid");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanNodeKind {
    Milestone,
    Packet,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanNode {
    pub node_id: String,
    pub project_id: String,
    pub kind: PlanNodeKind,
    pub version: u64,
    pub required: bool,
    #[serde(default)]
    pub criterion_refs: Vec<CriterionId>,
    /// Materials this node reads. Every entry must stay inside the plan read scope, so a
    /// proposal cannot silently widen the reading range of an admitted plan.
    #[serde(default)]
    pub input_refs: Vec<String>,
}

impl PlanNode {
    fn validate(&self, project_id: &str) -> Result<(), &'static str> {
        required(&self.node_id)?;
        required(&self.project_id)?;
        if self.project_id != project_id || self.version == 0 {
            return Err("plan_node_project_or_version_invalid");
        }
        let mut refs = BTreeSet::new();
        if self
            .criterion_refs
            .iter()
            .any(|criterion| !refs.insert(*criterion))
        {
            return Err("plan_node_criterion_duplicate");
        }
        let mut inputs = BTreeSet::new();
        if self
            .input_refs
            .iter()
            .any(|reference| read_reference(reference).is_err() || !inputs.insert(reference))
        {
            return Err("plan_node_input_reference_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanEdgeKind {
    ParentChild,
    RequiresArtifact,
    RequiresRunSuccess,
    RequiresAcceptance,
    Related,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanEdge {
    pub from: String,
    pub to: String,
    pub kind: PlanEdgeKind,
    #[serde(default)]
    pub reference: Option<String>,
}

impl PlanEdge {
    fn validate(&self) -> Result<(), &'static str> {
        required(&self.from)?;
        required(&self.to)?;
        if self.from == self.to {
            return Err("plan_edge_self_reference");
        }
        if !matches!(self.kind, PlanEdgeKind::ParentChild | PlanEdgeKind::Related)
            && self
                .reference
                .as_deref()
                .is_none_or(|value| required(value).is_err())
        {
            return Err("plan_edge_reference_required");
        }
        if let Some(reference) = self.reference.as_deref() {
            required(reference)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanProposal {
    pub schema: String,
    pub project_id: String,
    pub charter_baseline_version: u64,
    pub plan_version: u64,
    pub nodes: BTreeMap<String, PlanNode>,
    pub edges: Vec<PlanEdge>,
    /// Criterion identities the plan commits to deliver. Each one has to be owned by a required
    /// node, otherwise the criterion is isolated from the blocking graph.
    #[serde(default)]
    pub required_criteria: Vec<CriterionId>,
    /// Reading range the proposal may use. Node input references outside it fail closed.
    #[serde(default)]
    pub read_scope: Vec<String>,
    pub coverage_digest: String,
    pub digest: String,
}

impl PlanProposal {
    pub fn new(
        project_id: impl Into<String>,
        charter_baseline_version: u64,
        coverage_digest: impl Into<String>,
        nodes: Vec<PlanNode>,
        edges: Vec<PlanEdge>,
    ) -> Result<Self, &'static str> {
        let mut draft = PlanProposalDraft::new();
        for node in nodes {
            draft.push_node(node)?;
        }
        for edge in edges {
            draft.push_edge(edge)?;
        }
        draft.freeze(project_id, charter_baseline_version, coverage_digest)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        required(&self.project_id)?;
        if self.schema != PLAN_PROPOSAL_SCHEMA
            || self.charter_baseline_version == 0
            || self.plan_version == 0
        {
            return Err("plan_schema_or_version_invalid");
        }
        digest(&self.coverage_digest, "plan_coverage_digest_invalid")?;
        if self.nodes.is_empty() || self.nodes.len() > MAX_NODES {
            return Err("plan_nodes_required_or_too_large");
        }
        for (id, node) in &self.nodes {
            if id != &node.node_id {
                return Err("plan_node_key_mismatch");
            }
            node.validate(&self.project_id)?;
        }
        if self.edges.len() > MAX_EDGES {
            return Err("plan_edges_too_large");
        }
        let mut edge_ids = BTreeSet::new();
        for edge in &self.edges {
            edge.validate()?;
            if !edge_ids.insert((edge.from.clone(), edge.to.clone(), edge.kind)) {
                return Err("plan_edge_duplicate");
            }
            let from = self.nodes.get(&edge.from).ok_or("plan_edge_from_missing")?;
            let to = self.nodes.get(&edge.to).ok_or("plan_edge_to_missing")?;
            if edge.kind == PlanEdgeKind::ParentChild
                && (from.kind != PlanNodeKind::Milestone || to.kind != PlanNodeKind::Packet)
            {
                return Err("plan_parent_child_kind_invalid");
            }
        }
        if has_cycle(&self.nodes, &self.edges) {
            return Err("plan_dependency_cycle");
        }
        for (id, node) in &self.nodes {
            if node.required
                && !self.edges.iter().any(|edge| {
                    edge.from == *id || (edge.to == *id && edge.kind != PlanEdgeKind::Related)
                })
            {
                return Err("plan_required_node_isolated");
            }
        }
        self.validate_required_milestones()?;
        self.validate_required_criteria()?;
        self.validate_read_scope()?;
        if self.digest != self.canonical_digest() {
            return Err("plan_digest_mismatch");
        }
        Ok(())
    }

    /// A required Milestone is only independently acceptable when it owns at least one required
    /// Packet output; an empty milestone is a planning hole, not a boundary.
    fn validate_required_milestones(&self) -> Result<(), &'static str> {
        for (id, node) in &self.nodes {
            if !node.required || node.kind != PlanNodeKind::Milestone {
                continue;
            }
            let has_output = self.edges.iter().any(|edge| {
                edge.kind == PlanEdgeKind::ParentChild
                    && edge.from == *id
                    && self
                        .nodes
                        .get(&edge.to)
                        .is_some_and(|child| child.required && child.kind == PlanNodeKind::Packet)
            });
            if !has_output {
                return Err("plan_required_milestone_empty");
            }
        }
        Ok(())
    }

    /// Every declared required criterion must be owned by a required node; a criterion that only
    /// lives on optional nodes is isolated from what the plan commits to deliver.
    fn validate_required_criteria(&self) -> Result<(), &'static str> {
        if self.required_criteria.len() > MAX_REQUIRED_CRITERIA {
            return Err("plan_required_criteria_too_large");
        }
        let mut seen = BTreeSet::new();
        for criterion in &self.required_criteria {
            if !seen.insert(*criterion) {
                return Err("plan_required_criterion_duplicate");
            }
            let owned = self
                .nodes
                .values()
                .any(|node| node.required && node.criterion_refs.contains(criterion));
            if !owned {
                return Err("plan_required_criterion_isolated");
            }
        }
        Ok(())
    }

    /// The read scope is a closed set: every node input must be declared, entries are typed,
    /// bounded and unique.
    fn validate_read_scope(&self) -> Result<(), &'static str> {
        if self.read_scope.len() > MAX_READ_SCOPE {
            return Err("plan_read_scope_too_large");
        }
        let mut scope = BTreeSet::new();
        for reference in &self.read_scope {
            read_reference(reference)?;
            if !scope.insert(reference) {
                return Err("plan_read_scope_reference_duplicate");
            }
        }
        for node in self.nodes.values() {
            for reference in &node.input_refs {
                if !scope.contains(reference) {
                    return Err("plan_read_scope_insufficient");
                }
            }
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "project_id": self.project_id,
            "charter_baseline_version": self.charter_baseline_version,
            "plan_version": self.plan_version,
            "nodes": self.nodes,
            "edges": self.edges,
            "required_criteria": self.required_criteria,
            "read_scope": self.read_scope,
            "coverage_digest": self.coverage_digest,
        }))
    }

    pub fn topological_order(&self) -> Result<Vec<String>, &'static str> {
        self.validate()?;
        let mut indegree = self
            .nodes
            .keys()
            .map(|id| (id.clone(), 0usize))
            .collect::<BTreeMap<_, _>>();
        let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for edge in self
            .edges
            .iter()
            .filter(|edge| edge.kind != PlanEdgeKind::Related)
        {
            *indegree.get_mut(&edge.to).ok_or("plan_edge_to_missing")? += 1;
            outgoing
                .entry(edge.from.clone())
                .or_default()
                .push(edge.to.clone());
        }
        let mut ready = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(id.clone()))
            .collect::<BTreeSet<_>>();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = ready.pop_first() {
            order.push(id.clone());
            if let Some(children) = outgoing.get(&id) {
                for child in children {
                    let degree = indegree.get_mut(child).ok_or("plan_edge_to_missing")?;
                    *degree -= 1;
                    if *degree == 0 {
                        ready.insert(child.clone());
                    }
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err("plan_dependency_cycle");
        }
        Ok(order)
    }

    /// Deterministic blocking adjacency over resolvable endpoints; `Related` edges stay
    /// informational and never block.
    fn blocking_adjacency(&self) -> BTreeMap<String, Vec<String>> {
        let mut adjacency: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for edge in self
            .edges
            .iter()
            .filter(|edge| edge.kind != PlanEdgeKind::Related)
        {
            if self.nodes.contains_key(&edge.from) && self.nodes.contains_key(&edge.to) {
                adjacency
                    .entry(edge.from.clone())
                    .or_default()
                    .insert(edge.to.clone());
            }
        }
        adjacency
            .into_iter()
            .map(|(id, children)| (id, children.into_iter().collect()))
            .collect()
    }

    /// Stable cycle path for the blocking subgraph.
    ///
    /// The walk starts at the smallest node id and visits children in ascending order, so the
    /// same edge set always yields the same path regardless of insertion order. The first entry
    /// is repeated as the last entry (`a -> b -> a`). Unresolvable edges are reported by
    /// [`Self::missing_references`] instead.
    pub fn cycle_path(&self) -> Option<Vec<String>> {
        let adjacency = self.blocking_adjacency();
        let mut status: BTreeMap<String, u8> = BTreeMap::new();
        let mut path: Vec<String> = Vec::new();
        for id in self.nodes.keys() {
            if let Some(cycle) = walk_cycle(id, &adjacency, &mut status, &mut path) {
                return Some(cycle);
            }
        }
        None
    }

    /// Deterministic partial order over the resolvable blocking edges.
    fn resolvable_topological_order(&self) -> Vec<String> {
        let adjacency = self.blocking_adjacency();
        let mut indegree = self
            .nodes
            .keys()
            .map(|id| (id.clone(), 0usize))
            .collect::<BTreeMap<_, _>>();
        for children in adjacency.values() {
            for child in children {
                if let Some(degree) = indegree.get_mut(child) {
                    *degree += 1;
                }
            }
        }
        let mut ready = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(id.clone()))
            .collect::<BTreeSet<_>>();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = ready.pop_first() {
            order.push(id.clone());
            if let Some(children) = adjacency.get(&id) {
                for child in children {
                    let Some(degree) = indegree.get_mut(child) else {
                        continue;
                    };
                    *degree -= 1;
                    if *degree == 0 {
                        ready.insert(child.clone());
                    }
                }
            }
        }
        order
    }

    /// Unresolved edge endpoints in stable edge order, so a batch caller can point at the exact
    /// edge that names a node the candidate graph does not define (yet).
    pub fn missing_references(&self) -> Vec<PlanMissingReference> {
        let mut missing = Vec::new();
        for (edge_index, edge) in self.edges.iter().enumerate() {
            if !self.nodes.contains_key(&edge.from) {
                missing.push(PlanMissingReference {
                    kind: PlanReferenceKind::EdgeFrom,
                    edge_index,
                    reference: edge.from.clone(),
                    peer: edge.to.clone(),
                });
            }
            if !self.nodes.contains_key(&edge.to) {
                missing.push(PlanMissingReference {
                    kind: PlanReferenceKind::EdgeTo,
                    edge_index,
                    reference: edge.to.clone(),
                    peer: edge.from.clone(),
                });
            }
        }
        missing
    }

    /// Batch validation facts of the candidate graph: stable topological order, missing
    /// references, cycle path and the first rejection reason. It reads the raw fields, so it
    /// stays usable while the graph is not (yet) a valid proposal.
    pub fn graph_report(&self) -> PlanGraphReport {
        let missing_references = self.missing_references();
        let cycle_path = self.cycle_path();
        let order = self.resolvable_topological_order();
        let topological_order = (order.len() == self.nodes.len()).then_some(order);
        PlanGraphReport {
            schema: PLAN_GRAPH_REPORT_SCHEMA.to_owned(),
            project_id: self.project_id.clone(),
            plan_digest: self.digest.clone(),
            node_count: self.nodes.len(),
            edge_count: self.edges.len(),
            rejection: self.validate().err().map(str::to_owned),
            topological_order,
            cycle_path,
            missing_references,
        }
    }
}

/// Batch accumulator for a candidate plan graph.
///
/// Pushing a node or an edge never checks a topology, so a whole batch can be assembled in any
/// order (children before parents, edges before nodes). [`PlanProposalDraft::freeze`] runs the
/// complete validation exactly once, which is the approval boundary: a draft is never a plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanProposalDraft {
    pub schema: String,
    pub nodes: BTreeMap<String, PlanNode>,
    pub edges: Vec<PlanEdge>,
    #[serde(default)]
    pub required_criteria: Vec<CriterionId>,
    #[serde(default)]
    pub read_scope: Vec<String>,
}

impl Default for PlanProposalDraft {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanProposalDraft {
    pub fn new() -> Self {
        Self {
            schema: PLAN_DRAFT_SCHEMA.to_owned(),
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            required_criteria: Vec::new(),
            read_scope: Vec::new(),
        }
    }

    /// Push one node. Order never matters; an undefined endpoint is only rejected at freeze.
    pub fn push_node(&mut self, node: PlanNode) -> Result<(), &'static str> {
        if self.schema != PLAN_DRAFT_SCHEMA {
            return Err("plan_draft_schema_invalid");
        }
        required(&node.node_id)?;
        if self.nodes.len() >= MAX_NODES {
            return Err("plan_nodes_required_or_too_large");
        }
        if self.nodes.insert(node.node_id.clone(), node).is_some() {
            return Err("plan_node_duplicate");
        }
        Ok(())
    }

    /// Push one edge. Endpoints may not exist yet; duplicate edges are rejected immediately.
    pub fn push_edge(&mut self, edge: PlanEdge) -> Result<(), &'static str> {
        if self.schema != PLAN_DRAFT_SCHEMA {
            return Err("plan_draft_schema_invalid");
        }
        edge.validate()?;
        if self.edges.len() >= MAX_EDGES {
            return Err("plan_edges_too_large");
        }
        if self.edges.iter().any(|existing| {
            existing.from == edge.from && existing.to == edge.to && existing.kind == edge.kind
        }) {
            return Err("plan_edge_duplicate");
        }
        self.edges.push(edge);
        Ok(())
    }

    pub fn require_criterion(&mut self, criterion: CriterionId) -> Result<(), &'static str> {
        if self.required_criteria.contains(&criterion) {
            return Err("plan_required_criterion_duplicate");
        }
        if self.required_criteria.len() >= MAX_REQUIRED_CRITERIA {
            return Err("plan_required_criteria_too_large");
        }
        self.required_criteria.push(criterion);
        Ok(())
    }

    pub fn permit_read(&mut self, reference: impl Into<String>) -> Result<(), &'static str> {
        let reference = reference.into();
        read_reference(&reference)?;
        if self.read_scope.contains(&reference) {
            return Err("plan_read_scope_reference_duplicate");
        }
        if self.read_scope.len() >= MAX_READ_SCOPE {
            return Err("plan_read_scope_too_large");
        }
        self.read_scope.push(reference);
        Ok(())
    }

    /// Freeze the batch as revision 1 of the plan.
    pub fn freeze(
        self,
        project_id: impl Into<String>,
        charter_baseline_version: u64,
        coverage_digest: impl Into<String>,
    ) -> Result<PlanProposal, &'static str> {
        self.freeze_versioned(project_id, charter_baseline_version, 1, coverage_digest)
    }

    /// Freeze the batch as one exact plan revision; the full graph validation runs here.
    pub fn freeze_versioned(
        self,
        project_id: impl Into<String>,
        charter_baseline_version: u64,
        plan_version: u64,
        coverage_digest: impl Into<String>,
    ) -> Result<PlanProposal, &'static str> {
        if self.schema != PLAN_DRAFT_SCHEMA {
            return Err("plan_draft_schema_invalid");
        }
        if self.nodes.is_empty() {
            return Err("plan_nodes_required_or_too_large");
        }
        let Self {
            nodes,
            edges,
            required_criteria,
            read_scope,
            ..
        } = self;
        let mut proposal = PlanProposal {
            schema: PLAN_PROPOSAL_SCHEMA.to_owned(),
            project_id: project_id.into(),
            charter_baseline_version,
            plan_version,
            nodes,
            edges,
            required_criteria,
            read_scope,
            coverage_digest: coverage_digest.into(),
            digest: String::new(),
        };
        proposal.digest = proposal.canonical_digest();
        proposal.validate()?;
        Ok(proposal)
    }
}

/// Which endpoint of an edge names a node the candidate graph does not define.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanReferenceKind {
    EdgeFrom,
    EdgeTo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanMissingReference {
    pub kind: PlanReferenceKind,
    /// Index into the proposal edge list; stable because the report never reorders edges.
    pub edge_index: usize,
    /// The endpoint named by the edge that no node defines.
    pub reference: String,
    /// The other endpoint of the same edge, so the offending edge is locatable.
    pub peer: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanGraphReport {
    pub schema: String,
    pub project_id: String,
    pub plan_digest: String,
    pub node_count: usize,
    pub edge_count: usize,
    /// First rejection reason of the complete proposal validation, if any.
    pub rejection: Option<String>,
    /// Stable order over resolvable blocking edges; `None` when the graph is incomplete.
    pub topological_order: Option<Vec<String>>,
    pub cycle_path: Option<Vec<String>>,
    pub missing_references: Vec<PlanMissingReference>,
}

/// Frozen dependency versions of one admitted plan revision.
///
/// The freeze records the project, the plan/Charter versions, the coverage digest and the
/// per-node dependency versions at admission time. A later revision is compared against the
/// frozen set, so re-freezing cannot silently change which dependency version a downstream
/// consumer was promised.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenPlanGraph {
    pub schema: String,
    pub project_id: String,
    pub plan_digest: String,
    pub plan_version: u64,
    pub charter_baseline_version: u64,
    pub coverage_digest: String,
    /// `node_id -> dependency version` frozen at admission.
    pub node_versions: BTreeMap<String, u64>,
    pub digest: String,
}

impl FrozenPlanGraph {
    pub fn from_plan(plan: &PlanProposal) -> Result<Self, &'static str> {
        plan.validate()?;
        let node_versions = plan
            .nodes
            .iter()
            .map(|(id, node)| (id.clone(), node.version))
            .collect();
        let mut frozen = Self {
            schema: FROZEN_PLAN_GRAPH_SCHEMA.to_owned(),
            project_id: plan.project_id.clone(),
            plan_digest: plan.digest.clone(),
            plan_version: plan.plan_version,
            charter_baseline_version: plan.charter_baseline_version,
            coverage_digest: plan.coverage_digest.clone(),
            node_versions,
            digest: String::new(),
        };
        frozen.digest = frozen.canonical_digest();
        frozen.validate()?;
        Ok(frozen)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != FROZEN_PLAN_GRAPH_SCHEMA {
            return Err("frozen_plan_graph_schema_invalid");
        }
        required(&self.project_id)?;
        if self.plan_version == 0 || self.charter_baseline_version == 0 {
            return Err("frozen_plan_graph_version_invalid");
        }
        digest(&self.plan_digest, "frozen_plan_graph_plan_digest_invalid")?;
        digest(
            &self.coverage_digest,
            "frozen_plan_graph_coverage_digest_invalid",
        )?;
        if self.node_versions.is_empty() || self.node_versions.len() > MAX_NODES {
            return Err("frozen_plan_graph_nodes_invalid");
        }
        if self
            .node_versions
            .iter()
            .any(|(id, version)| required(id).is_err() || *version == 0)
        {
            return Err("frozen_plan_graph_node_versions_invalid");
        }
        if self.digest != self.canonical_digest() {
            return Err("frozen_plan_graph_freeze_digest_mismatch");
        }
        Ok(())
    }

    /// Verify a candidate plan revision against the frozen dependency versions.
    ///
    /// The refusal order is stable: project, frozen node set, node dependency version, plan
    /// version, Charter baseline, coverage digest and finally the whole-plan digest. A caller
    /// that only bumped a frozen dependency version gets that specific reason instead of a
    /// generic digest mismatch.
    pub fn verify(&self, plan: &PlanProposal) -> Result<(), &'static str> {
        self.validate()?;
        if plan.project_id != self.project_id {
            return Err("frozen_plan_graph_project_mismatch");
        }
        for (id, frozen_version) in &self.node_versions {
            let Some(node) = plan.nodes.get(id) else {
                return Err("frozen_plan_graph_node_missing");
            };
            if node.version != *frozen_version {
                return Err("frozen_plan_graph_node_version_mismatch");
            }
        }
        if plan
            .nodes
            .keys()
            .any(|id| !self.node_versions.contains_key(id))
        {
            return Err("frozen_plan_graph_node_unknown");
        }
        if plan.plan_version != self.plan_version {
            return Err("frozen_plan_graph_plan_version_mismatch");
        }
        if plan.charter_baseline_version != self.charter_baseline_version {
            return Err("frozen_plan_graph_baseline_mismatch");
        }
        if plan.coverage_digest != self.coverage_digest {
            return Err("frozen_plan_graph_coverage_mismatch");
        }
        if plan.digest != self.plan_digest {
            return Err("frozen_plan_graph_plan_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "project_id": self.project_id,
            "plan_digest": self.plan_digest,
            "plan_version": self.plan_version,
            "charter_baseline_version": self.charter_baseline_version,
            "coverage_digest": self.coverage_digest,
            "node_versions": self.node_versions,
        }))
    }
}

fn has_cycle(nodes: &BTreeMap<String, PlanNode>, edges: &[PlanEdge]) -> bool {
    let mut state = BTreeMap::new();
    for id in nodes.keys() {
        if visit(id, edges, &mut state) {
            return true;
        }
    }
    false
}

fn visit(id: &str, edges: &[PlanEdge], state: &mut BTreeMap<String, u8>) -> bool {
    match state.get(id).copied() {
        Some(1) => return true,
        Some(2) => return false,
        _ => {}
    }
    state.insert(id.to_owned(), 1);
    for edge in edges
        .iter()
        .filter(|edge| edge.kind != PlanEdgeKind::Related && edge.from == id)
    {
        if visit(&edge.to, edges, state) {
            return true;
        }
    }
    state.insert(id.to_owned(), 2);
    false
}

fn walk_cycle(
    id: &str,
    adjacency: &BTreeMap<String, Vec<String>>,
    status: &mut BTreeMap<String, u8>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    match status.get(id).copied() {
        Some(1) => {
            let start = path
                .iter()
                .position(|entry| entry == id)
                .unwrap_or_default();
            let mut cycle = path[start..].to_vec();
            cycle.push(id.to_owned());
            return Some(cycle);
        }
        Some(2) => return None,
        _ => {}
    }
    status.insert(id.to_owned(), 1);
    path.push(id.to_owned());
    if let Some(children) = adjacency.get(id) {
        for child in children {
            if let Some(cycle) = walk_cycle(child, adjacency, status, path) {
                return Some(cycle);
            }
        }
    }
    path.pop();
    status.insert(id.to_owned(), 2);
    None
}
