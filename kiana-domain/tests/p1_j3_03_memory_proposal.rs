use kiana_domain::{
    EventId, MemoryAdmission, MemoryEvidence, MemoryFact, MemoryOrigin, MemoryProposal,
    MemorySuggestion, SessionId,
};
use serde_json::json;

fn proposal() -> MemoryProposal {
    MemoryProposal {
        schema: kiana_domain::MEMORY_PROPOSAL_SCHEMA.to_owned(),
        id: "proposal-1".to_owned(),
        origin: MemoryOrigin::Model,
        admission_state: MemoryAdmission::Candidate,
        project_root: "/tmp/project".to_owned(),
        role_id: "pm".to_owned(),
        department_id: "planning".to_owned(),
        session_id: SessionId::new("session-1"),
        extractor: "llm.memory-distillation.v1".to_owned(),
        facts: vec![MemoryFact {
            kind: "lesson".to_owned(),
            operation: MemorySuggestion::Add,
            collection: "department:planning".to_owned(),
            text: "keep the acceptance evidence close to the decision".to_owned(),
            evidence: vec![MemoryEvidence {
                event_id: EventId::new(),
                request_id: kiana_domain::RequestId::new(),
                run_id: Some(kiana_domain::RunId::new()),
                quote: "acceptance evidence".to_owned(),
            }],
            similar_records: vec![json!({
                "id": "old-1",
                "collection": "department:planning",
                "similarity_score": 2
            })],
            target_record_id: None,
        }],
    }
}

#[test]
fn extraction_proposals_carry_evidence_and_similar_records() {
    let candidate = proposal();
    candidate.validate().unwrap();
    assert_eq!(candidate.admission_state, MemoryAdmission::Candidate);
    assert!(!candidate.facts[0].evidence.is_empty());
    assert!(candidate.facts[0].similar_records.len() <= 3);

    let mut missing_evidence = candidate.clone();
    missing_evidence.facts[0].evidence.clear();
    assert_eq!(
        missing_evidence.validate().unwrap_err(),
        "memory_proposal_fact_invalid"
    );

    let mut too_many_similar = candidate;
    too_many_similar.facts[0].similar_records = (0..4).map(|i| json!({"id": i})).collect();
    assert_eq!(
        too_many_similar.validate().unwrap_err(),
        "memory_proposal_fact_invalid"
    );
}

#[test]
fn proposal_operations_require_targets_except_add() {
    let mut update = proposal();
    update.facts[0].operation = MemorySuggestion::Update;
    assert_eq!(
        update.validate().unwrap_err(),
        "memory_proposal_target_required"
    );
    update.facts[0].target_record_id = Some("old-1".to_owned());
    update.validate().unwrap();
}
