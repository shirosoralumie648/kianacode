#[test]
fn rate_card_store_is_pinned_and_unknown_price_is_not_authority() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let pricing = include_str!("../../kiana-domain/src/billing_pricing.rs");
    let store_start = ports
        .find("pub trait RateCardStore")
        .expect("rate-card port start missing");
    let store_end = ports
        .find("/// Reservation/CAS boundary")
        .expect("rate-card port end missing");
    assert!(store_start < store_end);
    let rate_card_port = &ports[store_start..store_end];
    for marker in [
        "RateCardStore",
        "InMemoryRateCardStore",
        "resolve_rate_card",
        "rate_card_effective_overlap",
        "rate_card_unknown_model",
        "rate_card_expired",
        "read_rate_card",
        "rate_card_id.as_uuid().is_nil",
        "at_unix_ms == 0",
        "rate_card_lookup_invalid",
        "MAX_RATE_CARD_STORE_CARDS",
        "rate_card_capacity_exceeded",
    ] {
        assert!(
            rate_card_port.contains(marker),
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
            !rate_card_port.contains(forbidden),
            "rate-card port boundary widened: {forbidden}"
        );
    }
}
