use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}
fn usage(provider: Option<&str>) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        provider.map(str::to_owned),
        "model-a",
        "connection-a",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(3),
            output_tokens: Some(2),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        UsageConfidence::Known,
        None,
        "provider fixture",
        digest('a'),
    )
    .expect("fixture usage")
}

#[test]
fn missing_provider_or_rate_card_never_becomes_zero_cost() {
    assert!(ProviderUsageSettlement::from_usage(&usage(None), None, 1, 100).is_err());
    let settlement = ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
        .expect("unknown settlement");
    assert!(matches!(settlement.cost, SettlementCost::Unknown { .. }));
    assert_eq!(settlement.state, BillingState::Unknown);
}

#[test]
fn settlement_binds_only_an_explicit_non_nil_execution_identity() {
    let usage = usage(Some("provider-a"));
    let execution_id = ExecutionId::new();
    let settlement =
        ProviderUsageSettlement::from_usage_with_execution_id(&usage, execution_id, None, 1, 100)
            .expect("server-bound execution settlement");
    assert_eq!(settlement.execution_id, Some(execution_id));
    settlement.validate().expect("valid execution binding");
    let unbound = ProviderUsageSettlement::from_usage(&usage, None, 1, 100)
        .expect("compatible run/attempt settlement");
    assert_ne!(settlement.settlement_digest, unbound.settlement_digest);

    assert!(ProviderUsageSettlement::from_usage_with_execution_id(
        &usage,
        ExecutionId::from_uuid(uuid::Uuid::nil()),
        None,
        1,
        100,
    )
    .is_err());
}

#[test]
fn measured_receipt_is_explicit_and_attempt_replay_is_idempotent() {
    let mut settlement =
        ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
            .expect("unknown settlement");
    settlement
        .attach_measured(
            Money::new("USD", 17).expect("money"),
            ProviderReceiptRef::new("invoice-1").expect("receipt"),
        )
        .expect("measured settlement");
    let mut ledger = ProviderUsageLedger::default();
    assert_eq!(
        ledger.apply(settlement.clone()).expect("first apply"),
        SettlementApplyOutcome::Applied
    );
    assert_eq!(
        ledger.apply(settlement).expect("duplicate apply"),
        SettlementApplyOutcome::Duplicate
    );
}

#[test]
fn unpriced_nonzero_usage_dimension_keeps_known_observation_unknown() {
    let mut usage = usage(Some("provider-a"));
    usage.vector.tool_calls = 1;
    usage.usage_digest = usage.digest();
    usage.validate().expect("usage with tool call");

    let mut card = RateCard::new(
        RateCardId::new(),
        "provider-a",
        "model-a",
        "USD",
        Some(2),
        Some(3),
        1,
        None,
        1,
        "fixture-rate-card",
    )
    .expect("rate card");
    card.request_price = Some(5);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid rate card");

    let settlement =
        ProviderUsageSettlement::from_usage(&usage, Some(&card), 1, 100).expect("settlement");
    assert!(matches!(settlement.cost, SettlementCost::Unknown { .. }));
    assert_eq!(settlement.state, BillingState::Unknown);
}

#[test]
fn unknown_cost_transitions_fail_without_mutating_the_settlement() {
    let mut settlement =
        ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
            .expect("unknown settlement");
    let before = settlement.clone();

    assert_eq!(
        settlement.transition(BillingState::Corrected).unwrap_err(),
        "provider_usage_unknown_cannot_correct"
    );
    assert_eq!(settlement, before);

    assert_eq!(
        settlement.transition(BillingState::Settled).unwrap_err(),
        "provider_usage_unknown_cannot_settle"
    );
    assert_eq!(settlement, before);
}

#[test]
fn known_cost_cannot_be_downgraded_to_unknown() {
    let mut settlement =
        ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
            .expect("unknown settlement");
    settlement
        .attach_measured(
            Money::new("USD", 17).expect("money"),
            ProviderReceiptRef::new("invoice-2").expect("receipt"),
        )
        .expect("measured settlement");
    let before = settlement.clone();

    assert_eq!(
        settlement.transition(BillingState::Unknown).unwrap_err(),
        "billing_state_transition_invalid"
    );
    assert_eq!(settlement, before);
}

#[test]
fn invalid_measured_provider_receipt_is_rejected_after_deserialization() {
    let settlement = ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
        .expect("unknown settlement");
    let mut value = serde_json::to_value(&settlement).expect("serialize settlement");
    value["cost"] = serde_json::json!({
        "Measured": {
            "amount": {
                "schema": "kiana.money.v1",
                "currency": "USD",
                "micros": 17
            },
            "provider_receipt": "provider-receipt:secret"
        }
    });
    let mut injected: ProviderUsageSettlement =
        serde_json::from_value(value).expect("deserialize injected settlement");
    injected.settlement_digest = injected.digest();
    assert_eq!(injected.validate().unwrap_err(), "provider_receipt_invalid");
}
