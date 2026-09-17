//! Deterministic source-dependency graph and governance-epoch invalidation.
//!
//! The graph is a read-only index over committed Memory/Context provenance. A source revocation
//! advances the data epoch and reports the reverse dependency closure; it does not delete facts,
//! grant access, or silently rewrite a derived artifact.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const SOURCE_DEPENDENCY_GRAPH_SCHEMA: &str = "kiana.source-dependency-graph.v1";
pub const SOURCE_INVALIDATION_SCHEMA: &str = "kiana.source-invalidation.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyNodeKind {
    Memory,
    Evidence,
    Event,
    Artifact,
    File,
    ContextPlan,
    Index,
    Summary,
    Plan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyNode {
    pub id: String,
    pub kind: DependencyNodeKind,
    #[serde(default)]
    pub source_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyEdge {
    /// `from` is a derived node; `to` is the source/dependency it consumes.
    pub from: String,
    pub to: String,
    pub relation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInvalidation {
    pub schema: String,
    pub source_id: String,
    pub previous_epoch: u64,
    pub data_epoch: u64,
    pub affected_node_ids: Vec<String>,
    pub graph_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDependencyGraph {
    pub schema: String,
    pub data_epoch: u64,
    pub nodes: BTreeMap<String, DependencyNode>,
    pub edges: Vec<DependencyEdge>,
    pub graph_digest: String,
}

impl SourceDependencyGraph {
    pub fn new(
        data_epoch: u64,
        nodes: Vec<DependencyNode>,
        mut edges: Vec<DependencyEdge>,
    ) -> Result<Self, String> {
        if data_epoch == 0 || nodes.len() > 16_384 || edges.len() > 64 * 1024 {
            return Err("source_dependency_graph_header_invalid".to_owned());
        }
        let mut indexed = BTreeMap::new();
        for node in nodes {
            validate_node(&node)?;
            if indexed.insert(node.id.clone(), node).is_some() {
                return Err("source_dependency_node_duplicate".to_owned());
            }
        }
        for edge in &edges {
            validate_edge(edge, &indexed)?;
        }
        edges.sort_by(|left, right| {
            (&left.from, &left.to, &left.relation).cmp(&(&right.from, &right.to, &right.relation))
        });
        if edges.windows(2).any(|pair| {
            pair[0].from == pair[1].from
                && pair[0].to == pair[1].to
                && pair[0].relation == pair[1].relation
        }) {
            return Err("source_dependency_edge_duplicate".to_owned());
        }
        let mut graph = Self {
            schema: SOURCE_DEPENDENCY_GRAPH_SCHEMA.to_owned(),
            data_epoch,
            nodes: indexed,
            edges,
            graph_digest: String::new(),
        };
        graph.graph_digest = graph.digest();
        graph.validate()?;
        Ok(graph)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SOURCE_DEPENDENCY_GRAPH_SCHEMA || self.data_epoch == 0 {
            return Err("source_dependency_graph_header_invalid".to_owned());
        }
        if self.nodes.len() > 16_384 || self.edges.len() > 64 * 1024 {
            return Err("source_dependency_graph_limit".to_owned());
        }
        for node in self.nodes.values() {
            validate_node(node)?;
        }
        for edge in &self.edges {
            validate_edge(edge, &self.nodes)?;
        }
        if self.edges.windows(2).any(|pair| {
            (&pair[0].from, &pair[0].to, &pair[0].relation)
                > (&pair[1].from, &pair[1].to, &pair[1].relation)
        }) {
            return Err("source_dependency_edge_order_invalid".to_owned());
        }
        if self.graph_digest != self.digest() {
            return Err("source_dependency_graph_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "data_epoch": self.data_epoch,
            "nodes": self.nodes,
            "edges": self.edges,
        }))
    }

    /// Return derived nodes that transitively consume a source node or source_id.
    pub fn affected_by_source(&self, source_id: &str) -> Result<Vec<String>, String> {
        if source_id.trim().is_empty() || source_id.len() > 512 || source_id.contains('\0') {
            return Err("source_dependency_source_invalid".to_owned());
        }
        let seeds = self
            .nodes
            .values()
            .filter(|node| node.id == source_id || node.source_id.as_deref() == Some(source_id))
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        if seeds.is_empty() {
            return Err("source_dependency_source_not_found".to_owned());
        }
        let mut reverse = BTreeMap::<String, Vec<String>>::new();
        for edge in &self.edges {
            reverse
                .entry(edge.to.clone())
                .or_default()
                .push(edge.from.clone());
        }
        let mut seen = seeds.clone();
        let mut queue = VecDeque::from_iter(seeds);
        while let Some(id) = queue.pop_front() {
            for dependent in reverse.get(&id).into_iter().flatten() {
                if seen.insert(dependent.clone()) {
                    queue.push_back(dependent.clone());
                }
            }
        }
        Ok(seen.into_iter().collect())
    }

    /// Advance the governance epoch and return the exact invalidation closure.
    pub fn revoke_source(
        &mut self,
        source_id: &str,
        next_epoch: u64,
    ) -> Result<SourceInvalidation, String> {
        let affected = self.affected_by_source(source_id)?;
        if next_epoch <= self.data_epoch {
            return Err("source_dependency_epoch_rollback".to_owned());
        }
        let previous_epoch = self.data_epoch;
        self.data_epoch = next_epoch;
        self.graph_digest = self.digest();
        self.validate()?;
        Ok(SourceInvalidation {
            schema: SOURCE_INVALIDATION_SCHEMA.to_owned(),
            source_id: source_id.to_owned(),
            previous_epoch,
            data_epoch: next_epoch,
            affected_node_ids: affected,
            graph_digest: self.graph_digest.clone(),
        })
    }
}

fn validate_node(node: &DependencyNode) -> Result<(), String> {
    if node.id.trim().is_empty() || node.id.len() > 512 || node.id.contains('\0') {
        return Err("source_dependency_node_invalid".to_owned());
    }
    if node.source_id.as_deref().is_some_and(|source| {
        source.trim().is_empty() || source.len() > 512 || source.contains('\0')
    }) {
        return Err("source_dependency_source_invalid".to_owned());
    }
    Ok(())
}

fn validate_edge(
    edge: &DependencyEdge,
    nodes: &BTreeMap<String, DependencyNode>,
) -> Result<(), String> {
    if edge.from.trim().is_empty()
        || edge.to.trim().is_empty()
        || edge.relation.trim().is_empty()
        || edge.from.len() > 512
        || edge.to.len() > 512
        || edge.relation.len() > 128
        || !nodes.contains_key(&edge.from)
        || !nodes.contains_key(&edge.to)
        || edge.from == edge.to
    {
        return Err("source_dependency_edge_invalid".to_owned());
    }
    Ok(())
}
