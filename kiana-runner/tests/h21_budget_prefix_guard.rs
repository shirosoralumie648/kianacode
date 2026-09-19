#[test]
fn h21_runner_budget_and_prefix_inputs_are_observable_and_not_split() {
    let harness = include_str!("../src/harness.rs");
    let budget = include_str!("../src/budget.rs");
    let compact = include_str!("../src/compact.rs");
    let domain = include_str!("../../kiana-domain/src/request_budget.rs");
    for marker in [
        "TokenBudget",
        "reserved_output_tokens",
        "tool_schema_bytes",
        "prompt_sources",
        "route_digest",
        "StablePrefix",
        "ConservativeUtf8",
        "wire_budget_exceeded",
    ] {
        assert!(
            harness.contains(marker)
                || budget.contains(marker)
                || compact.contains(marker)
                || domain.contains(marker),
            "H21 budget/prefix marker missing: {marker}"
        );
    }
    assert!(compact.contains("Product-owned system instructions are an immutable prefix"));
    assert!(domain.contains("dynamic_suffix_digest"));
}
