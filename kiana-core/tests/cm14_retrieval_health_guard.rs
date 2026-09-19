#[test]
fn retrieval_results_keep_generation_provenance_and_safe_health_semantics() {
    let source = include_str!("../../kiana-domain/src/retrieval_evidence.rs");
    let candidate = include_str!("../../kiana-domain/src/retrieval_context.rs");
    for marker in [
        "RetrievalEvidence",
        "source_snapshot_digest",
        "source_revision",
        "generation",
        "rank_components",
        "RetrievalHealthStatus",
        "Denied",
        "Unavailable",
        "retryable",
        "retry_safe",
        "empty_reason",
        "RetrievalResponse",
        "Freshness",
        "EvidenceStatus",
    ] {
        assert!(
            source.contains(marker) || candidate.contains(marker),
            "CM-14 marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}
