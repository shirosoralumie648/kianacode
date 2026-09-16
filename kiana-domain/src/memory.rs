//! Memory admission and deterministic relevance. Missing provenance never becomes approval.
use crate::{
    MemoryCollection, Purpose, Retention, MEMORY_LAYER_INSTANCE_SCRATCH, MEMORY_RECORD_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const MEMORY_RECORD_SCHEMA_V2: &str = "kiana.memory-record.v2";
pub const MEMORY_REVIEW_SCHEMA: &str = "kiana.memory-review.v1";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySensitivity {
    #[default]
    Unknown,
    Public,
    Internal,
    Confidential,
    Restricted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryValidity {
    #[serde(default)]
    pub valid_from_ms: Option<u64>,
    #[serde(default)]
    pub valid_to_ms: Option<u64>,
}

impl MemoryValidity {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.valid_from_ms == Some(0)
            || self.valid_to_ms == Some(0)
            || matches!((self.valid_from_ms, self.valid_to_ms), (Some(from), Some(to)) if from > to)
        {
            return Err("memory_validity_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryImportMode {
    #[default]
    Native,
    LegacyImport,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<Purpose>,
    #[serde(default)]
    pub sensitivity: MemorySensitivity,
    #[serde(default)]
    pub validity: MemoryValidity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<Retention>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub import_mode: MemoryImportMode,
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
    /// Validate lifecycle/provenance combinations before a record is exposed to search or review.
    pub fn validate_lifecycle(&self) -> Result<(), String> {
        if self.schema != MEMORY_RECORD_SCHEMA && self.schema != MEMORY_RECORD_SCHEMA_V2 {
            return Err("memory_record_schema_unsupported".to_owned());
        }
        if self.id.trim().is_empty()
            || self.collection.trim().is_empty()
            || self.text.trim().is_empty()
        {
            return Err("memory_record_identity_invalid".to_owned());
        }
        let collection = MemoryCollection::parse(&self.collection)
            .ok_or_else(|| "memory_record_collection_invalid".to_owned())?;
        if self.layer != collection.layer {
            return Err("memory_record_layer_invalid".to_owned());
        }
        if self.schema == MEMORY_RECORD_SCHEMA_V2 && self.kind.trim().is_empty() {
            return Err("memory_record_kind_required".to_owned());
        }
        self.validity.validate().map_err(str::to_owned)?;
        if let Some(retention) = &self.retention {
            retention.validate()?;
        }
        if self.dependencies.len() > 64
            || self.dependencies.iter().any(|dependency| {
                dependency.trim().is_empty() || dependency.len() > 512 || dependency.contains('\0')
            })
        {
            return Err("memory_record_dependencies_invalid".to_owned());
        }
        if self.import_mode == MemoryImportMode::LegacyImport
            && (self.origin != MemoryOrigin::Unknown
                || self.admission_state != MemoryAdmission::Candidate
                || self.state != MemoryState::Draft
                || self.reviewed_by.is_some()
                || self.reviewed_at_ms.is_some())
        {
            return Err("memory_legacy_import_provenance_invalid".to_owned());
        }
        match (self.admission_state, self.state) {
            (MemoryAdmission::Candidate, MemoryState::Draft)
            | (MemoryAdmission::Qualified, MemoryState::Active)
            | (MemoryAdmission::Ephemeral, MemoryState::Active)
            | (MemoryAdmission::Rejected, MemoryState::Rejected) => {}
            _ => return Err("memory_admission_state_invalid".to_owned()),
        }
        if self.admission_state == MemoryAdmission::Qualified
            && (self.origin == MemoryOrigin::Unknown || self.evidence.is_empty())
        {
            return Err("memory_qualified_provenance_invalid".to_owned());
        }
        if self.admission_state == MemoryAdmission::Qualified
            && self.state == MemoryState::Active
            && self.import_mode != MemoryImportMode::LegacyImport
            && (self.origin == MemoryOrigin::Unknown
                || self.purpose.is_none()
                || self.sensitivity == MemorySensitivity::Unknown
                || self.reviewed_by.is_none()
                || self.reviewed_at_ms.is_none())
        {
            return Err("memory_active_qualification_incomplete".to_owned());
        }
        if self.admission_state == MemoryAdmission::Rejected && self.state != MemoryState::Rejected
        {
            return Err("memory_rejected_state_conflict".to_owned());
        }
        Ok(())
    }

    /// Explicitly import a legacy v1 row. Missing provenance is never upgraded to approved or
    /// verified; the caller must perform a new review before promotion.
    pub fn legacy_import(raw: Value) -> Result<Self, String> {
        let mut record: Self =
            serde_json::from_value(raw).map_err(|_| "memory_legacy_import_invalid".to_owned())?;
        record.schema = MEMORY_RECORD_SCHEMA_V2.to_owned();
        record.origin = MemoryOrigin::Unknown;
        record.admission_state = MemoryAdmission::Candidate;
        record.state = MemoryState::Draft;
        record.reviewed_by = None;
        record.review_reason = None;
        record.reviewed_at_ms = None;
        record.purpose = None;
        record.sensitivity = MemorySensitivity::Unknown;
        record.validity = MemoryValidity::default();
        record.retention = None;
        record.dependencies.clear();
        record.import_mode = MemoryImportMode::LegacyImport;
        if record.kind.trim().is_empty() {
            record.kind = "legacy_import".to_owned();
        }
        record.validate_lifecycle()?;
        Ok(record)
    }

    pub fn searchable(&self) -> bool {
        if self.import_mode == MemoryImportMode::LegacyImport {
            return false;
        }
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
            // Qualification/review and provenance are separate dimensions; this projection does
            // not manufacture an independent verification conclusion.
            "verified":false,
            "import_mode":self.import_mode,
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
