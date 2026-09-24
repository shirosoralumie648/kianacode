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
    let settlement =
        ProviderUsageSettlement::from_usage(&usage(Some("provider-a")), None, 1, 100)
            .expect("unknown settlement");
    assert!(matches!(settlement.cost, SettlementCost::Unknown { .. }));
    assert_eq!(settlement.state, BillingState::Unknown);
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
