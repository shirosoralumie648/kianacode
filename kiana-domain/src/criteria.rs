//! Versioned Company criterion coverage graphs.
//!
//! A coverage graph is a planning fact. It records which stable criterion at the Project level
//! is refined or covered by a Milestone/Packet criterion and which criterion has concrete
//! verification evidence. It never approves work or executes a test; ControlPlane remains the
//! authority for publishing and consuming a graph.
use crate::{json_digest, AcceptanceTarget, Criterion, CriterionId};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const CRITERIA_COVERAGE_SCHEMA: &str = "kiana.criteria-coverage.v1";
const MAX_CRITERIA: usize = 1_024;
const MAX_LINKS: usize = 4_096;

fn required(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("criteria_field_required_or_too_large")
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

    pub fn uncovered_required(&self) -> Vec<CriterionId> {
        self.entries
            .iter()
            .filter_map(|(id, entry)| {
                if entry.criterion.required
                    && !self.pending_required.contains(id)
                    && !self.links.iter().any(|link| {
                        link.parent_id == *id
                            && matches!(
                                link.relation,
                                CriterionRelation::Covers | CriterionRelation::Refines
                            )
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
