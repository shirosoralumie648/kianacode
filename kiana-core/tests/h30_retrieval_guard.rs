#[test]
fn retrieval_adapters_enter_one_context_plan_and_not_a_second_authority_path() {
    let domain = include_str!("../../kiana-domain/src/retrieval_context.rs");
    let plan = include_str!("../../kiana-domain/src/context_plan.rs");
    let query = include_str!("../../kiana-query/src/lib.rs");
    let adapter = include_str!("../../kiana-query/src/context_inputs.rs");
    let core = include_str!("../src/context_query.rs");
    for marker in [
        "RetrievalCandidate",
        "SourceSnapshot",
        "permission_scope",
        "acl_digest",
        "Freshness",
        "evidence_refs",
        "as_context_candidate",
        "ContextPlan::compile",
    ] {
        assert!(
            domain.contains(marker) || plan.contains(marker),
            "missing H30 marker: {marker}"
        );
    }
    assert!(query.contains("context_inputs"));
    assert!(adapter.contains("RetrievalCandidate"));
    assert!(core.contains("authorize_and_execute"));
    assert!(!domain.contains("PromptAuthority::Product"));
    assert!(!adapter.contains("CapabilityGrant"));
}
