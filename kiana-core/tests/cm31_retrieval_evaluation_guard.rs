#[test]
fn retrieval_evaluation_separates_determinism_quality_and_safety() {
    let domain = include_str!("../../kiana-domain/src/retrieval_evaluation.rs");
    let golden = include_str!("../../kiana-domain/src/golden_context.rs");
    let retrieval = include_str!("../../kiana-domain/src/unified_retrieval.rs");

    for marker in [
        "RETRIEVAL_EVALUATION_SCHEMA",
        "recall_at_k",
        "reciprocal_rank",
        "ndcg_at_k",
        "citation_precision",
        "freshness_rate",
        "duplicate_rate",
        "p95_latency_ms",
        "budget_overflow_rate",
        "unauthorized_hits",
        "revoked_hits",
        "stale_reinjected",
        "unverifiable_citations",
        "FixtureDeterminism",
        "SemanticModel",
        "semantic_quality_measured",
        "safety_passed",
        "golden_context",
        "rank_retrieval",
    ] {
        assert!(
            domain.contains(marker) || golden.contains(marker) || retrieval.contains(marker),
            "CM-31 source marker missing: {marker}"
        );
    }

    assert!(domain.contains("self.safety.validate"));
    assert!(domain.contains("semantic_quality_measured"));
    assert!(!domain.contains("ModelClient"));
    assert!(!domain.contains("CapabilityBroker"));
}
