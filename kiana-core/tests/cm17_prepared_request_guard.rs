#[test]
fn prepared_request_is_one_snapshot_and_rechecks_bindings() {
    let source = include_str!("../../kiana-domain/src/context_plan.rs");
    for marker in [
        "PreparedModelRequest",
        "pub scope: ScopeSet",
        "pub context_plan: ContextPlan",
        "pub source_snapshots: Vec<SourceSnapshot>",
        "pub wire_budget: WireBudget",
        "resolved_context_scope_changed",
        "resolved_context_route_changed",
        "request_digest",
        "rendered_prompt",
    ] {
        assert!(
            source.contains(marker),
            "CM-17 source marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}
