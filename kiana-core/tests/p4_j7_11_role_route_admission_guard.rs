#[test]
fn provider_attempt_admission_is_role_route_permit_and_budget_bound() {
    let domain = include_str!("../../kiana-domain/src/versioning.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let ports = include_str!("../../kiana-ports/src/model.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let budget = include_str!("../../kiana-domain/src/budget_contracts.rs");
    for marker in [
        "RouteDecision",
        "PreparedModelCall",
        "ModelCallPermit",
        "attempt_id",
        "request_hash",
        "route_digest",
        "expires_at_unix_ms",
        "consume_prepared",
        "role_id",
        "model_profile",
    ] {
        assert!(
            domain.contains(marker)
                || model.contains(marker)
                || ports.contains(marker)
                || provider.contains(marker)
                || budget.contains(marker),
            "missing P4-J7-11 marker: {marker}"
        );
    }
    assert!(!provider.contains("prompt_text_selects_provider"));
}
