//! Memory admission and deterministic relevance. Missing provenance never becomes approval.
use crate::{MemoryCollection, MEMORY_LAYER_INSTANCE_SCRATCH};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const MEMORY_RECORD_SCHEMA_V2: &str = "kiana.memory-record.v2";
pub const MEMORY_REVIEW_SCHEMA: &str = "kiana.memory-review.v1";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOrigin {
    #[default]
    Unknown,
    Model,
    Hook,
    Git,
    User,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAdmission {
    #[default]
    Candidate,
    Qualified,
    Ephemeral,
    Rejected,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryState {
    #[default]
    Draft,
    Active,
    Rejected,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryClassification {
    #[default]
    Unknown,
    Company,
    Department,
    Role,
    Project,
    UserPrivate,
    Scratch,
}
impl MemoryClassification {
    pub fn for_collection(collection: &MemoryCollection) -> Self {
        match collection.layer.as_str() {
            "company" => Self::Company,
            "department" => Self::Department,
            "role" => Self::Role,
            "project" => Self::Project,
            "user" => Self::UserPrivate,
            "instance-scratch" => Self::Scratch,
            _ => Self::Unknown,
        }
    }
}
fn first_revision() -> u64 {
    1
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemoryRecord {
    #[serde(default)]
    pub project_root: String,
    pub schema: String,
    pub id: String,
    pub layer: String,
    pub collection: String,
    pub text: String,
    pub source: String,
    pub role_id: String,
    pub department_id: String,
    pub session_id: String,
    pub created_at_ms: u64,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub evidence: Vec<crate::MemoryEvidence>,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub origin: MemoryOrigin,
    #[serde(default)]
    pub admission_state: MemoryAdmission,
    #[serde(default)]
    pub state: MemoryState,
    #[serde(default)]
    pub classification: MemoryClassification,
    #[serde(default = "first_revision")]
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at_ms: Option<u64>,
}
impl MemoryRecord {
    pub fn searchable(&self) -> bool {
        if self.state == MemoryState::Rejected || self.admission_state == MemoryAdmission::Rejected
        {
            return false;
        }
        if self.layer == MEMORY_LAYER_INSTANCE_SCRATCH {
            return true;
        }
        self.admission_state == MemoryAdmission::Qualified && self.state == MemoryState::Active
    }
    pub fn hit(&self) -> Value {
        json!({"project_root":self.project_root,"id":self.id, "layer":self.layer, "collection":self.collection,
            "source":self.source, "text":self.text, "origin":self.origin,
            "admission_state":self.admission_state, "state":self.state,
            "classification":self.classification, "revision":self.revision,
            "created_at_ms":self.created_at_ms, "reviewed_by":self.reviewed_by,
            "kind":self.kind,"evidence":self.evidence,"content_hash":self.content_hash,"supersedes":self.supersedes,
            "verified":false,
            "provenance":if self.origin == MemoryOrigin::Unknown || self.source.trim().is_empty() { "unverifiable" } else { "attributed" }})
    }
}
/// Unicode word tokens and overlapping CJK bigrams, in input order.
pub fn memory_tokens(text: &str) -> Vec<String> {
    fn cjk(ch: char) -> bool {
        matches!(ch as u32,0x3400..=0x4dbf|0x4e00..=0x9fff|0x20000..=0x3134f)
    }
    fn word_flush(word: &mut String, tokens: &mut Vec<String>) {
        if !word.is_empty() {
            tokens.push(std::mem::take(word));
        }
    }
    fn cjk_flush(run: &mut Vec<char>, tokens: &mut Vec<String>) {
        if run.len() == 1 {
            tokens.push(run[0].to_string());
        } else {
            for pair in run.windows(2) {
                tokens.push(pair.iter().collect());
            }
        }
        run.clear();
    }
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut run = Vec::new();
    for ch in text.chars().flat_map(char::to_lowercase) {
        if cjk(ch) {
            word_flush(&mut word, &mut tokens);
            run.push(ch);
        } else {
            cjk_flush(&mut run, &mut tokens);
            if ch.is_alphanumeric() || ch == '_' {
                word.push(ch);
            } else {
                word_flush(&mut word, &mut tokens);
            }
        }
    }
    word_flush(&mut word, &mut tokens);
    cjk_flush(&mut run, &mut tokens);
    tokens
}
pub fn memory_query_terms(query: &str) -> Vec<String> {
    memory_tokens(query)
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
pub fn memory_match_terms(text: &str, terms: &[String]) -> Vec<String> {
    let text = text.to_lowercase();
    terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .cloned()
        .collect()
}
pub fn memory_score_match(text: &str, terms: &[String]) -> u64 {
    memory_match_terms(text, terms).len() as u64
}
