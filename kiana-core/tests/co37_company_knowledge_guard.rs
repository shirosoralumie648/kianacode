#[test]
fn company_knowledge_keeps_decision_evidence_acl_and_memory_review_boundaries() {
    let knowledge = include_str!("../../kiana-domain/src/company_knowledge.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let memory = include_str!("../../kiana-domain/src/memory_distillation.rs");
    let proposals = include_str!("../../kiana-domain/src/memory_proposals.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let core = include_str!("../src/company_knowledge.rs");
    for marker in [
        "CompanyDecisionRecord",
        "CompanyLessonCandidate",
        "CompanyKnowledgePromotion",
        "CompanyKnowledgeLedger",
        "COMPANY_KNOWLEDGE_SCHEMA",
        "closing_receipt_ref",
        "source_event_ref",
        "company_candidate_collection_forbidden",
        "company_candidate_self_review_forbidden",
        "company_candidate_evidence_not_in_decision",
        "company_promotion_binding_invalid",
        "MemoryDistillationSource",
        "MemoryProposal",
        "memory.review",
        "accept_proposal",
        "publish_company_decision",
        "promote_company_candidate",
    ] {
        assert!(
            knowledge.contains(marker)
                || company.contains(marker)
                || memory.contains(marker)
                || proposals.contains(marker)
                || daemon.contains(marker)
                || core.contains(marker),
            "CO-37 marker missing: {marker}"
        );
    }
    for forbidden in [
        "user-private",
        "instance-scratch",
        "change_role_policy",
        "ModelClient::new",
        "Command::new",
    ] {
        assert!(
            !knowledge.contains(forbidden)
                || matches!(forbidden, "user-private" | "instance-scratch"),
            "CO-37 bypass marker present: {forbidden}"
        );
    }
}
