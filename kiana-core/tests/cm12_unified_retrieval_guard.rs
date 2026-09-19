#[test]
fn unified_retrieval_keeps_acl_first_and_one_algorithm_profile() {
    let domain = include_str!("../../kiana-domain/src/unified_retrieval.rs");
    let query = include_str!("../../kiana-query/src/unified_retrieval.rs");
    let daemon = include_str!("../../kiana-daemon/src/memory_retrieval.rs");
    let commands = include_str!("../../kiana-commands/src/memory.rs");
    for marker in [
        "RetrievalProfile",
        "RetrievalRequest",
        "RetrievalItem",
        "acl_filtered",
        "permission_scope_digest",
        "bm25_k1",
        "dense_vector",
        "rrf_k",
        "mmr_lambda",
        "acl_filtered_count",
        "exact_score",
        "sparse_rank",
        "dense_rank",
    ] {
        assert!(
            domain.contains(marker) || query.contains(marker),
            "CM-12 marker missing: {marker}"
        );
    }
    assert!(daemon.contains("rank_records"));
    assert!(commands.contains("search_memory_records"));
    assert!(!domain.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}
