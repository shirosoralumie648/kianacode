//! Memory proposals preserve quotations and never confer write or execution authority.
use crate::{
    memory_match_terms, memory_query_terms, prompt_hash, EventId, MemoryAdmission,
    MemoryCollection, MemoryOrigin, RequestId, RunId, RuntimeEvent, SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const MEMORY_PROPOSAL_SCHEMA: &str = "kiana.memory-proposal.v1";
pub const MEMORY_DISTILLATION_SCHEMA: &str = "kiana.memory-distillation.v1";
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MemorySuggestion {
    Add,
    Update,
    Delete,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEvidence {
    pub event_id: EventId,
    pub request_id: RequestId,
    pub run_id: Option<RunId>,
    pub quote: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryFact {
    pub kind: String,
    pub operation: MemorySuggestion,
    pub collection: String,
    pub text: String,
    pub evidence: Vec<MemoryEvidence>,
    pub similar_records: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_record_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryProposal {
    pub schema: String,
    pub id: String,
    pub origin: MemoryOrigin,
    pub admission_state: MemoryAdmission,
    pub project_root: String,
    pub role_id: String,
    pub department_id: String,
    pub session_id: SessionId,
    pub extractor: String,
    pub facts: Vec<MemoryFact>,
}
impl MemoryProposal {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != MEMORY_PROPOSAL_SCHEMA
            || self.id.trim().is_empty()
            || !matches!(self.origin, MemoryOrigin::Model | MemoryOrigin::Hook)
            || self.admission_state != MemoryAdmission::Candidate
            || self.facts.is_empty()
            || self.facts.len() > 8
        {
            return Err("memory_proposal_invalid");
        }
        for fact in &self.facts {
            if !matches!(
                fact.kind.as_str(),
                "fact" | "decision" | "lesson" | "preference" | "event"
            ) || MemoryCollection::parse(&fact.collection).is_none()
                || fact.text.trim().is_empty()
                || fact.text.len() > 16 * 1024
                || fact.evidence.is_empty()
                || fact.evidence.len() > 8
                || fact.similar_records.len() > 3
            {
                return Err("memory_proposal_fact_invalid");
            }
            if fact
                .evidence
                .iter()
                .any(|e| e.quote.trim().is_empty() || e.quote.len() > 16 * 1024)
            {
                return Err("memory_proposal_evidence_required");
            }
            if fact.operation != MemorySuggestion::Add
                && fact
                    .target_record_id
                    .as_ref()
                    .is_none_or(|id| id.trim().is_empty())
            {
                return Err("memory_proposal_target_required");
            }
        }
        Ok(())
    }
}
/// Produces an attributed quotation candidate, without claiming that a model inferred a lesson.
pub fn quoted_memory_proposal(
    source: &RuntimeEvent,
    scope: (&str, &str, &str, &SessionId),
    run_id: Option<RunId>,
    kind: &str,
    text: &str,
    events: &[RuntimeEvent],
) -> Result<Option<MemoryProposal>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let quote = text.chars().take(2048).collect::<String>();
    let terms = memory_query_terms(&quote);
    let mut candidates: BTreeMap<String, Value> = BTreeMap::new();
    for event in events.iter().filter(|e| e.kind == "capability.completed") {
        if let Some(hits) = event.data.get("hits").and_then(Value::as_array) {
            for hit in hits {
                let Some(id) = hit.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(text) = hit.get("text").and_then(Value::as_str) else {
                    continue;
                };
                if text == quote {
                    return Ok(None);
                }
                let score = memory_match_terms(text, &terms).len();
                if score == 0 {
                    continue;
                }
                let mut hit = hit.clone();
                hit["similarity_score"] = json!(score);
                candidates.insert(id.to_owned(), hit);
            }
        }
    }
    let mut similar = candidates.into_values().collect::<Vec<_>>();
    similar.sort_by(|a, b| {
        b["similarity_score"]
            .as_u64()
            .cmp(&a["similarity_score"].as_u64())
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    similar.truncate(3);
    let (project, role, department, session) = scope;
    let proposal = MemoryProposal {
        schema: MEMORY_PROPOSAL_SCHEMA.to_owned(),
        id: format!("proposal:{}:{}", source.event_id, prompt_hash(&quote)),
        origin: MemoryOrigin::Hook,
        admission_state: MemoryAdmission::Candidate,
        project_root: project.to_owned(),
        role_id: role.to_owned(),
        department_id: department.to_owned(),
        session_id: session.clone(),
        extractor: "deterministic_terminal_quote.v1".to_owned(),
        facts: vec![MemoryFact {
            kind: kind.to_owned(),
            operation: MemorySuggestion::Add,
            collection: format!("department:{department}"),
            text: quote.clone(),
            evidence: vec![MemoryEvidence {
                event_id: source.event_id,
                request_id: source.request_id,
                run_id,
                quote,
            }],
            similar_records: similar,
            target_record_id: None,
        }],
    };
    proposal.validate()?;
    Ok(Some(proposal))
}
