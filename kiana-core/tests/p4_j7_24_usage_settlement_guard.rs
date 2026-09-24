#[test]
fn usage_settlement_keeps_cost_and_authority_boundaries_separate() {
    let domain = include_str!("../../kiana-domain/src/provider_usage_settlement.rs");
    let provider = include_str!("../../kiana-provider/src/usage.rs");
    for marker in [
        "SettlementCost::Unknown",
        "RateCard",
        "attach_measured",
        "provider_usage_attempt_conflict",
        "BillingState::Unknown",
        "provider_usage_run_mismatch",
    ] {
        assert!(
            domain.contains(marker) || provider.contains(marker),
            "P4-J7-24 marker missing: {marker}"
        );
    }
    for forbidden in ["CapabilityBroker", "tokio::spawn", "reqwest::Client", "EventStore"] {
        assert!(
            !domain.contains(forbidden) && !provider.contains(forbidden),
            "usage settlement boundary widened: {forbidden}"
        );
    }
}
