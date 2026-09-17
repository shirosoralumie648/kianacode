#[test]
fn memory_hits_respect_knowledge_grants_and_reach_the_receipt() {
    let memory = include_str!("../src/harness_memory.rs");
    let retrieval = include_str!("../src/memory_retrieval.rs");
    let receipts = include_str!("../../kiana-core/src/receipts.rs");
    let existing = include_str!("daemon_host.rs");
    for marker in [
        "server_memory_scope",
        "scope.allows_collection",
        "role.allows_knowledge",
        "rank_records",
        "score_components",
        "retrieved_by",
        "retrieval_event_id",
        "memory_hits_from_events",
        "builder_project_search_hits_land_on_receipt",
    ] {
        assert!(
            memory.contains(marker)
                || retrieval.contains(marker)
                || receipts.contains(marker)
                || existing.contains(marker),
            "memory retrieval marker missing: {marker}"
        );
    }
    assert!(memory.contains("searchable()"));
    assert!(retrieval.contains("memory_match_terms"));
    assert!(receipts.contains("retrieval_request_id"));
}

#[test]
fn unauthorized_memory_collection_is_rejected_before_read() {
    let memory = include_str!("../src/harness_memory.rs");
    let roles = include_str!("../../kiana-domain/src/roles.rs");
    let existing = include_str!("daemon_host.rs");
    assert!(memory.contains("memory_scope_collection_denied"));
    assert!(memory.contains("role_knowledge_denied"));
    assert!(roles.contains("allows_knowledge"));
    assert!(existing.contains("builder_cannot_search_user_private_memory"));
    assert!(existing.contains("builder_cannot_search_unreleased_planning_debate"));
}
