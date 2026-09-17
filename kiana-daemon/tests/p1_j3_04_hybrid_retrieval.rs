#[test]
fn hybrid_retrieval_contract_is_pinned_and_fail_closed() {
    let retrieval = include_str!("../src/memory_retrieval.rs");
    let memory = include_str!("../../kiana-domain/src/memory.rs");
    let harness = include_str!("../src/harness_memory.rs");
    let receipts = include_str!("../../kiana-core/src/receipts.rs");
    for marker in [
        "Local deterministic BM25",
        "memory_tokens",
        "KIANA_MEMORY_EMBEDDING_MANIFEST",
        "kiana.embedding-model.v1",
        "memory_embedding_hash_mismatch",
        "memory_embedding_format_not_supported",
        "embedding_model_missing",
        "1.0 / (61.0 + rank as f64)",
        "0.7 * rrf[*i] / max_rrf - 0.3 * redundancy",
        "retrieval_algorithm",
        "score_components",
        "rank_records",
        "retrieval_session_id",
        "retrieval_event_id",
    ] {
        assert!(
            retrieval.contains(marker)
                || memory.contains(marker)
                || harness.contains(marker)
                || receipts.contains(marker),
            "hybrid retrieval marker missing: {marker}"
        );
    }
    assert!(memory.contains("overlapping CJK bigrams"));
    assert!(retrieval.contains("O_NOFOLLOW"));
    assert!(retrieval.contains("No network or hash-generated embeddings"));
    assert!(retrieval.contains("degraded_reason"));
    assert!(retrieval.contains("sha256"));
    assert!(!retrieval.contains("reqwest::"));
    assert!(!retrieval.contains("ort::"));
}
