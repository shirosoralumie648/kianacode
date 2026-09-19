#[test]
fn golden_context_contract_is_snapshot_and_algorithm_bound() {
    let domain = include_str!("../../kiana-domain/src/golden_context.rs");
    let retrieval = include_str!("../../kiana-domain/src/unified_retrieval.rs");
    let context = include_str!("../../kiana-domain/src/context_plan.rs");

    for marker in [
        "GoldenContextFixture",
        "GoldenContextCase",
        "GOLDEN_CONTEXT_FIXTURE_SCHEMA",
        "source_snapshot_digest",
        "policy_epoch",
        "data_epoch",
        "index_generation",
        "algorithm_digest",
        "embedding_digest",
        "context_plan_digest",
        "retrieval_result_digest",
        "golden_context_coverage_incomplete",
        "golden_retrieval_rank_mismatch",
        "canonical_bytes",
        "english",
        "chinese",
        "cjk",
        "identifier",
        "path",
        "time",
        "acl",
        "rank_retrieval",
        "ContextPlan",
    ] {
        assert!(
            domain.contains(marker) || retrieval.contains(marker) || context.contains(marker),
            "CM-30 source marker missing: {marker}"
        );
    }

    assert!(domain.contains("fixture_digest"));
    assert!(domain.contains("expected_ranked_ids"));
    assert!(!domain.contains("ModelClient"));
    assert!(!domain.contains("CapabilityBroker"));
}
