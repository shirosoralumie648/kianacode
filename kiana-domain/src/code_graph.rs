//! Provenance- and scope-bound code graph edges with temporal validity.
//!
//! The graph is a derived read structure. It can explain symbol/dependency relationships but it
//! never grants ACL, replaces source text, or mutates the EventLog. Deletion produces a bounded
//! rebuild plan for affected edges only.

use crate::{json_digest, SchemaVersion, SourceKind, SourceRef};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CODE_GRAPH_EDGE_SCHEMA: &str = "kiana.code-graph-edge.v1";
pub const CODE_GRAPH_REBUILD_SCHEMA: &str = "kiana.code-graph-rebuild.v1";
pub const CODE_GRAPH_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeGraphNodeKind {
    File,
    Symbol,
    Package,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeGraphRelation {
    Contains,
    Imports,
    Calls,
    References,
    Implements,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeGraphTemporalState {
    Valid,
    Invalid,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeGraphEdge {
    pub schema: String,
    pub version: SchemaVersion,
    pub edge_id: String,
    pub from_id: String,
    pub from_kind: CodeGraphNodeKind,
    pub to_id: String,
    pub to_kind: CodeGraphNodeKind,
    pub relation: CodeGraphRelation,
    pub source: SourceRef,
    pub scope_digest: String,
    pub valid_from_ms: u64,
    #[serde(default)]
    pub valid_to_ms: Option<u64>,
    #[serde(default)]
    pub invalidated_at_ms: Option<u64>,
    pub edge_digest: String,
}

impl CodeGraphEdge {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        edge_id: impl Into<String>,
        from_id: impl Into<String>,
        from_kind: CodeGraphNodeKind,
        to_id: impl Into<String>,
        to_kind: CodeGraphNodeKind,
        relation: CodeGraphRelation,
        source: SourceRef,
        scope_digest: impl Into<String>,
        valid_from_ms: u64,
        valid_to_ms: Option<u64>,
    ) -> Result<Self, String> {
        let mut edge = Self {
            schema: CODE_GRAPH_EDGE_SCHEMA.to_owned(),
            version: CODE_GRAPH_VERSION,
            edge_id: edge_id.into(),
            from_id: from_id.into(),
            from_kind,
            to_id: to_id.into(),
            to_kind,
            relation,
            source,
            scope_digest: scope_digest.into(),
            valid_from_ms,
            valid_to_ms,
            invalidated_at_ms: None,
            edge_digest: String::new(),
        };
        edge.edge_digest = edge.digest();
        edge.validate()?;
        Ok(edge)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODE_GRAPH_EDGE_SCHEMA
            || self.version != CODE_GRAPH_VERSION
            || self.valid_from_ms == 0
            || self
                .valid_to_ms
                .is_some_and(|value| value <= self.valid_from_ms)
            || self
                .invalidated_at_ms
                .is_some_and(|value| value <= self.valid_from_ms)
        {
            return Err("code_graph_edge_header_invalid".to_owned());
        }
        required(&self.edge_id, "code_graph_edge_id", 512)?;
        required(&self.from_id, "code_graph_edge_from", 512)?;
        required(&self.to_id, "code_graph_edge_to", 512)?;
        if self.from_id == self.to_id {
            return Err("code_graph_edge_self_reference".to_owned());
        }
        self.source.validate()?;
        if self.source.kind != SourceKind::WorkspaceFile {
            return Err("code_graph_edge_source_kind_invalid".to_owned());
        }
        digest(&self.scope_digest, "code_graph_edge_scope_digest")?;
        digest(&self.edge_digest, "code_graph_edge_digest")?;
        if self.edge_digest != self.digest() {
            return Err("code_graph_edge_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn state_at(&self, as_of_ms: u64) -> Result<CodeGraphTemporalState, String> {
        self.validate()?;
        if as_of_ms == 0 || as_of_ms < self.valid_from_ms {
            return Err("code_graph_edge_as_of_invalid".to_owned());
        }
        if self
            .invalidated_at_ms
            .is_some_and(|invalidated| invalidated <= as_of_ms)
        {
            return Ok(CodeGraphTemporalState::Invalid);
        }
        if self.valid_to_ms.is_some_and(|expired| expired <= as_of_ms) {
            return Ok(CodeGraphTemporalState::Expired);
        }
        Ok(CodeGraphTemporalState::Valid)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "edge_id": self.edge_id,
            "from_id": self.from_id,
            "from_kind": self.from_kind,
            "to_id": self.to_id,
            "to_kind": self.to_kind,
            "relation": self.relation,
            "source": self.source,
            "scope_digest": self.scope_digest,
            "valid_from_ms": self.valid_from_ms,
            "valid_to_ms": self.valid_to_ms,
            "invalidated_at_ms": self.invalidated_at_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeGraphRebuildPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_id: String,
    pub source_digest: String,
    pub data_epoch: u64,
    pub affected_edge_ids: Vec<String>,
    pub retained_edge_ids: Vec<String>,
    pub plan_digest: String,
}

impl CodeGraphRebuildPlan {
    pub fn for_source(
        edges: &[CodeGraphEdge],
        source_id: impl Into<String>,
        source_digest: impl Into<String>,
        data_epoch: u64,
    ) -> Result<Self, String> {
        let source_id = source_id.into();
        let source_digest = source_digest.into();
        if data_epoch == 0 {
            return Err("code_graph_rebuild_epoch_invalid".to_owned());
        }
        required(&source_id, "code_graph_rebuild_source_id", 512)?;
        digest(&source_digest, "code_graph_rebuild_source_digest")?;
        let mut affected = Vec::new();
        let mut retained = Vec::new();
        let mut ids = BTreeSet::new();
        for edge in edges {
            edge.validate()?;
            if !ids.insert(edge.edge_id.clone()) {
                return Err("code_graph_rebuild_edge_duplicate".to_owned());
            }
            if edge.source.source_id == source_id {
                affected.push(edge.edge_id.clone());
            } else {
                retained.push(edge.edge_id.clone());
            }
        }
        affected.sort();
        retained.sort();
        let mut plan = Self {
            schema: CODE_GRAPH_REBUILD_SCHEMA.to_owned(),
            version: CODE_GRAPH_VERSION,
            source_id,
            source_digest,
            data_epoch,
            affected_edge_ids: affected,
            retained_edge_ids: retained,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODE_GRAPH_REBUILD_SCHEMA
            || self.version != CODE_GRAPH_VERSION
            || self.data_epoch == 0
            || self
                .affected_edge_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self
                .retained_edge_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("code_graph_rebuild_plan_invalid".to_owned());
        }
        required(&self.source_id, "code_graph_rebuild_source_id", 512)?;
        digest(&self.source_digest, "code_graph_rebuild_source_digest")?;
        let affected = self.affected_edge_ids.iter().collect::<BTreeSet<_>>();
        if affected
            .intersection(&self.retained_edge_ids.iter().collect::<BTreeSet<_>>())
            .next()
            .is_some()
        {
            return Err("code_graph_rebuild_edge_overlap".to_owned());
        }
        digest(&self.plan_digest, "code_graph_rebuild_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("code_graph_rebuild_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_id": self.source_id,
            "source_digest": self.source_digest,
            "data_epoch": self.data_epoch,
            "affected_edge_ids": self.affected_edge_ids,
            "retained_edge_ids": self.retained_edge_ids,
        }))
    }
}
