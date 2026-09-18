use kiana_domain::{BillingUnknownReason, Money, RateCard, RateCardId, UsageVector};

fn card() -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider:test",
        "model:test",
        "USD",
        Some(2),
        Some(3),
        1_000,
        Some(10_000),
        1,
        "fixture-rate-card",
    )
    .unwrap();
    card.request_price = Some(5);
    card.tool_price = Some(7);
    card.rate_card_digest = card.digest();
    card.validate().unwrap();
    card
}

#[test]
fn money_and_rate_card_use_checked_integer_micros() {
    let money = Money::new("USD", 10).unwrap();
    assert_eq!(money.checked_mul_units(3).unwrap().micros, 30);
    assert_eq!(
        money
            .checked_add(&Money::new("USD", 5).unwrap())
            .unwrap()
            .micros,
        15
    );
    assert_eq!(
        money
            .checked_add(&Money::new("EUR", 5).unwrap())
            .unwrap_err(),
        "money_currency_mismatch"
    );
    assert!(Money::new("usd", 1).is_err());

    let mut usage = UsageVector::zero();
    usage.input_tokens = Some(10);
    usage.output_tokens = Some(4);
    usage.tool_calls = 2;
    let estimate = card().estimate(&usage, 1).unwrap();
    estimate.validate().unwrap();
    assert_eq!(estimate.amount.unwrap().micros, 10 * 2 + 4 * 3 + 7 * 2 + 5);
}

#[test]
fn missing_price_or_usage_is_explicit_unknown_and_effective_window_is_bounded() {
    let card = card();
    assert!(card.is_effective_at(1_000));
    assert!(!card.is_effective_at(10_000));
    let mut usage = UsageVector::zero();
    usage.input_tokens = None;
    let estimate = card.estimate(&usage, 0).unwrap();
    assert_eq!(estimate.amount, None);
    assert_eq!(estimate.unknown_reason, Some(BillingUnknownReason::Partial));

    let mut no_output_price = card;
    no_output_price.output_price_per_unit = None;
    no_output_price.rate_card_digest = no_output_price.digest();
    let mut output_only = UsageVector::zero();
    output_only.input_tokens = None;
    output_only.output_tokens = Some(5);
    let estimate = no_output_price.estimate(&output_only, 0).unwrap();
    assert_eq!(
        estimate.unknown_reason,
        Some(BillingUnknownReason::RateCardMissing)
    );
}

#[test]
fn rate_card_rejects_version_time_and_digest_drift() {
    let mut bad = card();
    bad.effective_to_unix_ms = Some(1_000);
    bad.rate_card_digest = bad.digest();
    assert_eq!(bad.validate().unwrap_err(), "rate_card_invalid");
    let mut tampered = card();
    tampered.input_price_per_unit = Some(u64::MAX);
    assert_eq!(tampered.validate().unwrap_err(), "rate_card_invalid");
}
