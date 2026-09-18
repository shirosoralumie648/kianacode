#[test]
fn rate_card_store_is_pinned_and_unknown_price_is_not_authority() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let pricing = include_str!("../../kiana-domain/src/billing_pricing.rs");
    for marker in [
        "RateCardStore",
        "InMemoryRateCardStore",
        "resolve_rate_card",
        "rate_card_effective_overlap",
        "rate_card_unknown_model",
        "rate_card_expired",
        "read_rate_card",
    ] {
        assert!(
            ports.contains(marker),
            "rate-card port marker missing: {marker}"
        );
    }
    for marker in [
        "BillingPriceDimension",
        "AudioInputTokens",
        "AudioOutputTokens",
        "Effects",
        "rate_card_version",
    ] {
        assert!(
            pricing.contains(marker),
            "price mapping marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest",
        "tokio::spawn",
        "CapabilityBroker",
        "FinancialBudget",
    ] {
        assert!(
            !ports.contains(forbidden),
            "rate-card port boundary widened: {forbidden}"
        );
    }
}
