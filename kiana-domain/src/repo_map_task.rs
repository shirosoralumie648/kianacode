//! Task-lens ranking and evidence contracts for RepoMap.
//!
//! Symbols and dependency edges are explicitly heuristic/declared evidence. This layer never
//! labels a text scan as a compiler fact and only selects read-only context material under a
//! deterministic token budget.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const REPO_MAP_SYMBOL_SCHEMA: &str = "kiana.repo-map-symbol.v1";
pub const REPO_MAP_DEPENDENCY_SCHEMA: &str = "kiana.repo-map-dependency.v1";
pub const REPO_MAP_TASK_SELECTION_SCHEMA: &str = "kiana.repo-map-task-selection.v1";
pub const REPO_MAP_TASK_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_REPO_MAP_CANDIDATES: usize = 4_096;
pub const MAX_REPO_MAP_SYMBOLS: usize = 32;
pub const MAX_REPO_MAP_EDGES: usize = 64;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoMapEvidenceLevel {
    Heuristic,
    Declared,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoMapSymbol {
    pub schema: String,
    pub name: String,
    pub evidence: RepoMapEvidenceLevel,
    pub symbol_digest: String,
}

impl RepoMapSymbol {
    pub fn heuristic(name: impl Into<String>) -> Result<Self, String> {
        let mut symbol = Self {
            schema: REPO_MAP_SYMBOL_SCHEMA.to_owned(),
            name: name.into(),
            evidence: RepoMapEvidenceLevel::Heuristic,
            symbol_digest: String::new(),
        };
        symbol.symbol_digest = symbol.digest();
        symbol.validate()?;
        Ok(symbol)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REPO_MAP_SYMBOL_SCHEMA {
            return Err("repo_map_symbol_schema_invalid".to_owned());
        }
        required(&self.name, "repo_map_symbol_name", 256)?;
        digest(&self.symbol_digest, "repo_map_symbol_digest")?;
        if self.symbol_digest != self.digest() {
            return Err("repo_map_symbol_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "name": self.name,
            "evidence": self.evidence,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoMapDependencyEdge {
    pub schema: String,
    pub source_path: String,
    pub target_path: String,
    pub relation: String,
    pub evidence: RepoMapEvidenceLevel,
    pub edge_digest: String,
}

impl RepoMapDependencyEdge {
    pub fn heuristic(
        source_path: impl Into<String>,
        target_path: impl Into<String>,
        relation: impl Into<String>,
    ) -> Result<Self, String> {
        let mut edge = Self {
            schema: REPO_MAP_DEPENDENCY_SCHEMA.to_owned(),
            source_path: source_path.into(),
            target_path: target_path.into(),
            relation: relation.into(),
            evidence: RepoMapEvidenceLevel::Heuristic,
            edge_digest: String::new(),
        };
        edge.edge_digest = edge.digest();
        edge.validate()?;
        Ok(edge)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REPO_MAP_DEPENDENCY_SCHEMA {
            return Err("repo_map_dependency_schema_invalid".to_owned());
        }
        required(&self.source_path, "repo_map_dependency_source", 4_096)?;
        required(&self.target_path, "repo_map_dependency_target", 4_096)?;
        required(&self.relation, "repo_map_dependency_relation", 128)?;
        digest(&self.edge_digest, "repo_map_dependency_digest")?;
        if self.edge_digest != self.digest() {
            return Err("repo_map_dependency_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source_path": self.source_path,
            "target_path": self.target_path,
            "relation": self.relation,
            "evidence": self.evidence,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoMapTaskCandidate {
    pub path: String,
    pub language: Option<String>,
    pub content_digest: String,
    pub symbols: Vec<RepoMapSymbol>,
    pub dependencies: Vec<RepoMapDependencyEdge>,
    pub task_score: u64,
    pub estimated_tokens: u64,
}

impl RepoMapTaskCandidate {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.path, "repo_map_candidate_path", 4_096)?;
        digest(&self.content_digest, "repo_map_candidate_content_digest")?;
        if self.symbols.len() > MAX_REPO_MAP_SYMBOLS || self.dependencies.len() > MAX_REPO_MAP_EDGES
        {
            return Err("repo_map_candidate_limit".to_owned());
        }
        let mut symbols = BTreeSet::new();
        for symbol in &self.symbols {
            symbol.validate()?;
            if !symbols.insert(symbol.name.clone()) {
                return Err("repo_map_candidate_symbol_duplicate".to_owned());
            }
        }
        let mut edges = BTreeSet::new();
        for edge in &self.dependencies {
            edge.validate()?;
            if !edges.insert(edge.edge_digest.clone()) {
                return Err("repo_map_candidate_dependency_duplicate".to_owned());
            }
        }
        if self.estimated_tokens == 0 {
            return Err("repo_map_candidate_token_estimate_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoMapTaskSelection {
    pub schema: String,
    pub version: SchemaVersion,
    pub task_digest: String,
    pub token_budget: u64,
    pub estimated_tokens: u64,
    pub selected: Vec<RepoMapTaskCandidate>,
    pub omitted_paths: Vec<String>,
    pub selection_digest: String,
}

impl RepoMapTaskSelection {
    pub fn new(
        task: impl Into<String>,
        token_budget: u64,
        mut candidates: Vec<RepoMapTaskCandidate>,
    ) -> Result<Self, String> {
        if token_budget == 0 || candidates.len() > MAX_REPO_MAP_CANDIDATES {
            return Err("repo_map_selection_header_invalid".to_owned());
        }
        for candidate in &candidates {
            candidate.validate()?;
        }
        candidates.sort_by(|left, right| {
            right
                .task_score
                .cmp(&left.task_score)
                .then(left.path.cmp(&right.path))
                .then(left.content_digest.cmp(&right.content_digest))
        });
        let mut selected = Vec::new();
        let mut omitted_paths = Vec::new();
        let mut estimated_tokens = 0;
        for candidate in candidates {
            if estimated_tokens.saturating_add(candidate.estimated_tokens) <= token_budget {
                estimated_tokens = estimated_tokens.saturating_add(candidate.estimated_tokens);
                selected.push(candidate);
            } else {
                omitted_paths.push(candidate.path);
            }
        }
        let mut selection = Self {
            schema: REPO_MAP_TASK_SELECTION_SCHEMA.to_owned(),
            version: REPO_MAP_TASK_VERSION,
            task_digest: json_digest(&json!({"task": task.into()})),
            token_budget,
            estimated_tokens,
            selected,
            omitted_paths,
            selection_digest: String::new(),
        };
        selection.selection_digest = selection.digest();
        selection.validate()?;
        Ok(selection)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REPO_MAP_TASK_SELECTION_SCHEMA
            || self.version != REPO_MAP_TASK_VERSION
            || self.token_budget == 0
            || self.estimated_tokens > self.token_budget
        {
            return Err("repo_map_selection_header_invalid".to_owned());
        }
        digest(&self.task_digest, "repo_map_selection_task_digest")?;
        let mut paths = BTreeSet::new();
        let mut total = 0_u64;
        for candidate in &self.selected {
            candidate.validate()?;
            if !paths.insert(candidate.path.clone()) {
                return Err("repo_map_selection_duplicate_path".to_owned());
            }
            total = total.saturating_add(candidate.estimated_tokens);
        }
        for path in &self.omitted_paths {
            required(path, "repo_map_selection_omitted_path", 4_096)?;
            if !paths.insert(path.clone()) {
                return Err("repo_map_selection_duplicate_path".to_owned());
            }
        }
        if total != self.estimated_tokens {
            return Err("repo_map_selection_token_mismatch".to_owned());
        }
        digest(&self.selection_digest, "repo_map_selection_digest")?;
        if self.selection_digest != self.digest() {
            return Err("repo_map_selection_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "task_digest": self.task_digest,
            "token_budget": self.token_budget,
            "estimated_tokens": self.estimated_tokens,
            "selected": self.selected,
            "omitted_paths": self.omitted_paths,
        }))
    }
}
