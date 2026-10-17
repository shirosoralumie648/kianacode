#[test]
fn hybrid_retrieval_contract_is_pinned_and_fail_closed() {
    let retrieval = include_str!("../src/memory_retrieval.rs");
    let memory = include_str!("../../kiana-domain/src/memory.rs");
    let unified = include_str!("../../kiana-domain/src/unified_retrieval.rs");
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
        // CM-12：RRF/MMR 公式已下沉到 Domain `rank_retrieval()`；这里钉统一实现里的公式与字段名。
        "1.0 / (profile.rrf_k as f64 + rank as f64 + 1.0)",
        "profile.mmr_lambda * base - (1.0 - profile.mmr_lambda) * redundancy",
        "retrieval_algorithm",
        "score_components",
        "rank_records",
        "retrieval_session_id",
        "retrieval_event_id",
    ] {
        assert!(
            retrieval.contains(marker)
                || memory.contains(marker)
                || unified.contains(marker)
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
    // `UnifiedRetrievalPort::` legitimately contains the `ort::` character run, so the ONNX check
    // has to be identifier-boundary aware; the intent (no `ort` crate path) is unchanged.
    assert!(
        !retrieval
            .match_indices("ort::")
            .any(|(index, _)| retrieval[..index]
                .chars()
                .next_back()
                .is_none_or(|previous| !(previous.is_ascii_alphanumeric() || previous == '_'))),
        "the local retrieval path must not link the ONNX runtime"
    );
}
