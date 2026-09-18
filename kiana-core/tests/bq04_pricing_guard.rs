#[test]
fn pricing_contracts_are_integer_checked_and_not_billing_authority() {
    let pricing = include_str!("../../kiana-domain/src/billing_pricing.rs");
    for marker in [
        "Money",
        "RateCard",
        "CostEstimate",
        "micros",
        "checked_add",
        "checked_mul",
        "effective_from_unix_ms",
        "effective_to_unix_ms",
        "rate_card_digest",
        "BillingUnknownReason::RateCardMissing",
    ] {
        assert!(pricing.contains(marker), "pricing marker missing: {marker}");
    }
    for forbidden in [
        "f64",
        "f32",
        "reqwest",
        "tokio",
        "std::fs",
        "CapabilityBroker",
        "InvoiceStore",
    ] {
        assert!(
            !pricing.contains(forbidden),
            "pricing boundary widened: {forbidden}"
        );
    }
}
