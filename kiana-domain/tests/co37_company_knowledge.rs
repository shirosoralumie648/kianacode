use kiana_domain::*;

fn decision() -> CompanyDecisionRecord {
    let mut value = CompanyDecisionRecord {
        schema: COMPANY_KNOWLEDGE_SCHEMA.to_owned(),
        decision_id: "decision-1".to_owned(),
        project_id: "project-1".to_owned(),
        department_id: "engineering".to_owned(),
        kind: CompanyDecisionKind::Closing,
        source_event_ref: "event:closing-1".to_owned(),
        closing_receipt_ref: "receipt:closing-1".to_owned(),
        summary: "keep the bounded local delivery gate".to_owned(),
        evidence_refs: vec!["evidence:closing-1".to_owned()],
        published_by: "sponsor-1".to_owned(),
        published_at: 10,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn candidate(collection: &str, scope: CompanyKnowledgeScope) -> CompanyLessonCandidate {
    let mut value = CompanyLessonCandidate {
        schema: COMPANY_KNOWLEDGE_SCHEMA.to_owned(),
        candidate_id: "candidate-1".to_owned(),
        decision_id: "decision-1".to_owned(),
        project_id: "project-1".to_owned(),
        department_id: "engineering".to_owned(),
        role_id: "builder".to_owned(),
        scope,
        collection: collection.to_owned(),
        kind: "lesson".to_owned(),
        text: "Keep accepted artifacts bound to one immutable manifest.".to_owned(),
        evidence_refs: vec!["evidence:closing-1".to_owned()],
        proposed_by: "distiller-1".to_owned(),
        proposed_at: 20,
        state: CompanyCandidateState::Candidate,
        reviewed_by: None,
        review_reason: None,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn promotion(candidate: &CompanyLessonCandidate) -> CompanyKnowledgePromotion {
    let mut value = CompanyKnowledgePromotion {
        schema: COMPANY_KNOWLEDGE_SCHEMA.to_owned(),
        promotion_id: "promotion-1".to_owned(),
        candidate_id: candidate.candidate_id.clone(),
        project_id: candidate.project_id.clone(),
        collection: candidate.collection.clone(),
        memory_record_ref: "memory:department:engineering:lesson-1".to_owned(),
        approved_by: candidate.reviewed_by.clone().unwrap(),
        evidence_refs: candidate.evidence_refs.clone(),
        promoted_at: 30,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn closing_lesson_cannot_publish_private_scratch_or_change_role_policy() {
    let mut ledger = CompanyKnowledgeLedger::default();
    ledger.publish_decision(decision()).expect("decision");
    let private = candidate("user-private", CompanyKnowledgeScope::Department);
    assert_eq!(
        ledger.propose_candidate(private).unwrap_err(),
        "company_candidate_collection_forbidden"
    );
    let mut wrong_evidence = candidate("department:engineering", CompanyKnowledgeScope::Department);
    wrong_evidence.evidence_refs = vec!["evidence:other".to_owned()];
    wrong_evidence.digest = wrong_evidence.canonical_digest();
    assert_eq!(
        ledger.propose_candidate(wrong_evidence).unwrap_err(),
        "company_candidate_evidence_not_in_decision"
    );
}

#[test]
fn duplicate_distillation_self_review_and_cross_project_candidates_are_rejected() {
    let mut ledger = CompanyKnowledgeLedger::default();
    ledger.publish_decision(decision()).expect("decision");
    let item = candidate("department:engineering", CompanyKnowledgeScope::Department);
    ledger.propose_candidate(item.clone()).expect("candidate");
    ledger
        .propose_candidate(item)
        .expect("idempotent candidate");
    assert_eq!(
        ledger
            .review_candidate("candidate-1", true, "distiller-1", "same author")
            .unwrap_err(),
        "company_candidate_self_review_forbidden"
    );
    let mut foreign = candidate("project:project-2", CompanyKnowledgeScope::Project);
    foreign.project_id = "project-2".to_owned();
    foreign.digest = foreign.canonical_digest();
    assert_eq!(
        ledger.propose_candidate(foreign).unwrap_err(),
        "company_candidate_binding_invalid"
    );
}

#[test]
fn approved_department_lesson_is_retrievable_with_original_decision_evidence() {
    let mut ledger = CompanyKnowledgeLedger::default();
    ledger.publish_decision(decision()).expect("decision");
    ledger
        .propose_candidate(candidate(
            "department:engineering",
            CompanyKnowledgeScope::Department,
        ))
        .expect("candidate");
    ledger
        .review_candidate(
            "candidate-1",
            true,
            "reviewer-1",
            "verified against receipt",
        )
        .expect("review");
    let approved = ledger.candidates["candidate-1"].clone();
    ledger.promote(promotion(&approved)).expect("promotion");
    let promoted = &ledger.promotions["promotion-1"];
    assert_eq!(promoted.collection, "department:engineering");
    assert_eq!(promoted.evidence_refs, ["evidence:closing-1"]);
    ledger
        .promote(promotion(&approved))
        .expect("idempotent promotion");
}
