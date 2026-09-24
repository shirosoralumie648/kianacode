use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

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
        7,
        "bq13-fixture",
    )
    .expect("rate card");
    card.request_price = Some(5);
    card.tool_price = Some(7);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid rate card");
    card
}

fn usage(confidence: UsageConfidence) -> NormalizedUsage {
    usage_with_run(RunId::new(), confidence)
}

fn usage_with_run(run_id: RunId, confidence: UsageConfidence) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        AttemptId::new(),
        run_id,
        Some("provider:test".to_owned()),
        "model:test",
        "route:test",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(10),
            output_tokens: Some(4),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        (confidence != UsageConfidence::Known).then_some(BillingUnknownReason::Partial),
        "provider fixture",
        digest('a'),
    )
    .expect("usage");
}

fn incomplete_usage() -> NormalizedUsage {
    let mut usage = usage(UsageConfidence::Known);
    usage.vector.output_tokens = None;
    usage.usage_digest = usage.digest();
    usage
}

#[test]
fn estimate_is_card_version_bound_and_has_dimension_lines() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let breakdown = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    assert!(matches!(
        breakdown.kind,
        CostBreakdownKind::Estimated { .. }
    ));
    assert_eq!(breakdown.rate_card_id, Some(card.rate_card_id));
    assert_eq!(breakdown.rate_card_version, Some(card.card_version));
    assert!(breakdown.lines.iter().any(|line| {
        line.dimension == BillingPriceDimension::InputTokens && line.amount.is_some()
    }));
    breakdown.validate().expect("valid breakdown");
}

#[test]
fn missing_card_or_partial_usage_is_unknown_and_never_zero() {
    let known = usage(UsageConfidence::Known);
    let missing = CostBreakdown::from_usage(&known, None, 1, 2_000).expect("unknown cost");
    assert!(matches!(
        missing.kind,
        CostBreakdownKind::Unknown {
            reason: BillingUnknownReason::RateCardMissing
        }
    ));
    assert!(missing.kind.amount().is_none());

    let partial = usage(UsageConfidence::Partial);
    let partial_cost =
        CostBreakdown::from_usage(&partial, Some(&card()), 1, 2_000).expect("partial unknown");
    assert!(matches!(
        partial_cost.kind,
        CostBreakdownKind::Unknown {
            reason: BillingUnknownReason::Partial
        }
    ));
    assert!(partial_cost.kind.amount().is_none());

    let incomplete = incomplete_usage();
    let incomplete_cost =
        CostBreakdown::from_usage(&incomplete, Some(&card()), 1, 2_000).expect("incomplete");
    assert!(matches!(
        incomplete_cost.kind,
        CostBreakdownKind::Unknown {
            reason: BillingUnknownReason::Partial
        }
    ));
}

#[test]
fn measured_requires_card_and_complete_usage_but_serializes_only_receipt() {
    let known = usage(UsageConfidence::Known);
    let receipt = ProviderReceiptRef::new("provider-receipt-1").expect("receipt");
    assert_eq!(
        CostBreakdown::from_provider_receipt(
            &known,
            None,
            Money::new("USD", 19).expect("money"),
            receipt.clone(),
            2_000,
        )
        .unwrap_err(),
        "cost_measured_rate_card_missing"
    );
    let partial = usage(UsageConfidence::Partial);
    assert_eq!(
        CostBreakdown::from_provider_receipt(
            &partial,
            Some(&card()),
            Money::new("USD", 19).expect("money"),
            receipt.clone(),
            2_000,
        )
        .unwrap_err(),
        "cost_measured_usage_incomplete"
    );
    let incomplete = incomplete_usage();
    assert_eq!(
        CostBreakdown::from_provider_receipt(
            &incomplete,
            Some(&card()),
            Money::new("USD", 19).expect("money"),
            receipt.clone(),
            2_000,
        )
        .unwrap_err(),
        "cost_measured_usage_incomplete"
    );

    let measured = CostBreakdown::from_provider_receipt(
        &known,
        Some(&card()),
        Money::new("USD", 19).expect("money"),
        receipt,
        2_000,
    )
    .expect("measured");
    assert!(matches!(measured.kind, CostBreakdownKind::Measured { .. }));
    assert!(measured.rate_card_id.is_none());
    let encoded = serde_json::to_value(&measured).expect("encoded");
    assert!(encoded.get("provider_receipt").is_none());
    assert_eq!(encoded["kind"]["provider_receipt"], "provider-receipt-1");
    assert!(encoded.get("rate_card_id").is_none());
}

#[test]
fn receipt_breakdown_keeps_estimate_measured_and_unknown_separate() {
    let first = usage(UsageConfidence::Known);
    let estimate = CostBreakdown::from_usage(&first, Some(&card()), 1, 2_000).expect("estimate");
    let second = usage_with_run(first.run_id, UsageConfidence::Known);
    let measured = CostBreakdown::from_provider_receipt(
        &second,
        Some(&card()),
        Money::new("USD", 31).expect("money"),
        ProviderReceiptRef::new("provider-receipt-2").expect("receipt"),
        2_000,
    )
    .expect("measured");
    let unknown = CostBreakdown::unknown(
        first.run_id,
        Some(AttemptId::new()),
        digest('b'),
        UsageConfidence::Unknown,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("unknown");
    let receipt = ReceiptCostBreakdown::from_entries(
        vec![estimate, measured, unknown],
        3,
        vec![EventId::new(), EventId::new(), EventId::new()],
    )
    .expect("receipt breakdown");
    assert!(receipt.estimated_total.is_some());
    assert_eq!(
        receipt.measured_total.as_ref().map(|money| money.micros),
        Some(31)
    );
    assert_eq!(
        receipt.unknown_reasons,
        vec![BillingUnknownReason::ResultUnknown]
    );
    receipt.validate().expect("valid receipt breakdown");
}

#[test]
fn receipt_breakdown_rejects_cross_run_entries() {
    let first = usage(UsageConfidence::Known);
    let second = usage(UsageConfidence::Known);
    let estimate = CostBreakdown::from_usage(&first, Some(&card()), 1, 2_000).expect("estimate");
    let measured = CostBreakdown::from_provider_receipt(
        &second,
        Some(&card()),
        Money::new("USD", 5).expect("money"),
        ProviderReceiptRef::new("provider-receipt-cross-run").expect("receipt"),
        2_000,
    )
    .expect("measured");
    assert_eq!(
        ReceiptCostBreakdown::from_entries(vec![estimate, measured], 1, vec![EventId::new()],)
            .unwrap_err(),
        "receipt_cost_run_mismatch"
    );
}

#[test]
fn cost_events_replay_latest_measured_without_double_counting_estimate() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let measured = CostBreakdown::from_provider_receipt(
        &usage,
        Some(&card),
        Money::new("USD", 29).expect("money"),
        ProviderReceiptRef::new("provider-receipt-3").expect("receipt"),
        2_000,
    )
    .expect("measured");
    let first = estimate
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let second = measured
        .to_runtime_event(RequestId::new(), 2)
        .expect("event");
    let projection = project_receipt_cost_breakdown(usage.run_id, &[first, second])
        .expect("projected")
        .expect("cost facts");
    assert_eq!(projection.entries.len(), 1);
    assert_eq!(projection.estimated_total, None);
    assert_eq!(
        projection.measured_total.as_ref().map(|money| money.micros),
        Some(29)
    );
}
