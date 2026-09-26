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
const MAX_NODES: usize = 1_024;
const MAX_EDGES: usize = 4_096;

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
        if nodes.is_empty() || nodes.len() > MAX_NODES {
            return Err("plan_nodes_required_or_too_large");
        }
        if edges.len() > MAX_EDGES {
            return Err("plan_edges_too_large");
        }
        let mut indexed = BTreeMap::new();
        for node in nodes {
            if indexed.insert(node.node_id.clone(), node).is_some() {
                return Err("plan_node_duplicate");
            }
        }
        let mut proposal = Self {
            schema: PLAN_PROPOSAL_SCHEMA.to_owned(),
            project_id: project_id.into(),
            charter_baseline_version,
            plan_version: 1,
            nodes: indexed,
            edges,
            coverage_digest: coverage_digest.into(),
            digest: String::new(),
        };
        proposal.digest = proposal.canonical_digest();
        proposal.validate()?;
        Ok(proposal)
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
        if self.digest != self.canonical_digest() {
            return Err("plan_digest_mismatch");
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
