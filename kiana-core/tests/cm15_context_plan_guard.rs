#[test]
fn context_plan_selection_keeps_material_and_authority_boundaries() {
    let plan = include_str!("../../kiana-domain/src/context_plan.rs");
    for marker in [
        "ContextMaterialType",
        "ProductSystem",
        "WorkspaceSnapshot",
        "LiveResult",
        "ContextSourceBudget",
        "context_source_budget_exceeded",
        "context_product_source_untrusted",
        "selection_priority",
        "source_allowed",
    ] {
        assert!(
            plan.contains(marker),
            "CM-15 source marker missing: {marker}"
        );
    }
    assert!(!plan.contains("ModelClient"));
    assert!(!plan.contains("CapabilityBroker"));
}
