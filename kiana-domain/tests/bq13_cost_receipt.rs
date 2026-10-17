use kiana_domain::*;
use serde_json::json;

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
    .expect("usage")
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
fn overlapping_usage_is_unknown_before_cost_lines_are_emitted() {
    let mut value = usage(UsageConfidence::Known);
    value.vector.cache_read_tokens = Some(4);
    value.vector.reasoning_output_tokens = Some(2);
    value.usage_digest = value.digest();
    let breakdown = CostBreakdown::from_usage(&value, Some(&card()), 1, 2_000).unwrap();
    assert!(matches!(
        breakdown.kind,
        CostBreakdownKind::Unknown {
            reason: BillingUnknownReason::Unsupported
        }
    ));
    assert!(breakdown.lines.is_empty());
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
    assert_eq!(
        encoded["kind"]["measured"]["provider_receipt"],
        "provider-receipt-1"
    );
    assert!(encoded.get("rate_card_id").is_none());
    let decoded: CostBreakdown = serde_json::from_value(encoded).expect("measured round trip");
    assert_eq!(decoded, measured);
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

#[test]
fn estimate_scope_drift_and_invalid_observation_are_refused() {
    let usage = usage(UsageConfidence::Known);
    let card = card();

    let mut foreign_provider = card.clone();
    foreign_provider.provider_id = "provider:other".to_owned();
    foreign_provider.rate_card_digest = foreign_provider.digest();
    assert_eq!(
        CostBreakdown::from_usage(&usage, Some(&foreign_provider), 1, 2_000).unwrap_err(),
        "cost_rate_card_scope_mismatch"
    );

    let mut foreign_model = card.clone();
    foreign_model.model_selector = "model:other".to_owned();
    foreign_model.rate_card_digest = foreign_model.digest();
    assert_eq!(
        CostBreakdown::from_usage(&usage, Some(&foreign_model), 1, 2_000).unwrap_err(),
        "cost_rate_card_scope_mismatch"
    );

    // The card is not yet effective at the observation instant.
    assert_eq!(
        CostBreakdown::from_usage(&usage, Some(&card), 1, 999).unwrap_err(),
        "cost_rate_card_scope_mismatch"
    );

    assert_eq!(
        CostBreakdown::from_usage(&usage, Some(&card), 0, 2_000).unwrap_err(),
        "cost_observation_invalid"
    );
    assert_eq!(
        CostBreakdown::from_usage(&usage, Some(&card), 1, 0).unwrap_err(),
        "cost_observation_invalid"
    );
}

#[test]
fn measured_rejects_negative_amount_and_scope_or_currency_drift() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let receipt = || ProviderReceiptRef::new("provider-receipt-fence").expect("receipt");

    assert_eq!(
        CostBreakdown::from_provider_receipt(
            &usage,
            Some(&card),
            Money::new("USD", -1).expect("money"),
            receipt(),
            2_000,
        )
        .unwrap_err(),
        "cost_measured_amount_negative"
    );
    assert_eq!(
        CostBreakdown::from_provider_receipt(
            &usage,
            Some(&card),
            Money::new("EUR", 19).expect("money"),
            receipt(),
            2_000,
        )
        .unwrap_err(),
        "cost_measured_currency_mismatch"
    );
    assert_eq!(
        CostBreakdown::from_provider_receipt(
            &usage,
            Some(&card),
            Money::new("USD", 19).expect("money"),
            receipt(),
            999,
        )
        .unwrap_err(),
        "cost_rate_card_scope_mismatch"
    );
    assert_eq!(
        CostBreakdown::from_provider_receipt(
            &usage,
            None,
            Money::new("USD", 19).expect("money"),
            receipt(),
            2_000,
        )
        .unwrap_err(),
        "cost_measured_rate_card_missing"
    );
}

#[test]
fn estimate_lines_use_checked_integer_micros() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let breakdown = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    assert_eq!(breakdown.kind.amount().map(|money| money.micros), Some(37));
    let input_line = breakdown
        .lines
        .iter()
        .find(|line| line.dimension == BillingPriceDimension::InputTokens)
        .expect("input line");
    assert_eq!(input_line.units, Some(10));
    assert_eq!(input_line.unit_price_micros, Some(2));
    assert_eq!(
        input_line.amount.as_ref().map(|money| money.micros),
        Some(20)
    );
    let request_line = breakdown
        .lines
        .iter()
        .find(|line| line.dimension == BillingPriceDimension::Requests)
        .expect("request line");
    assert_eq!(request_line.units, Some(1));
    assert_eq!(
        request_line.amount.as_ref().map(|money| money.micros),
        Some(5)
    );
    breakdown.validate().expect("valid breakdown");
}

#[test]
fn measured_zero_is_an_explicit_observation_and_unknown_never_carries_an_amount() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let measured = CostBreakdown::from_provider_receipt(
        &usage,
        Some(&card),
        Money::new("USD", 0).expect("money"),
        ProviderReceiptRef::new("provider-receipt-zero").expect("receipt"),
        2_000,
    )
    .expect("explicit zero receipt");
    assert!(measured.is_measured());
    assert_eq!(measured.kind.amount().map(|money| money.micros), Some(0));
    assert_eq!(
        measured.provider_receipt().map(ProviderReceiptRef::as_str),
        Some("provider-receipt-zero")
    );

    let unknown = CostBreakdown::from_usage(&usage, None, 1, 2_000).expect("unknown");
    assert!(unknown.kind.amount().is_none());
    let encoded = serde_json::to_value(&unknown).expect("encoded");
    assert!(encoded.get("provider_receipt").is_none());
    assert!(encoded["kind"]["unknown"].get("amount").is_none());
    assert_eq!(encoded["kind"]["unknown"]["reason"], "rate_card_missing");

    // A fabricated amount smuggled into the unknown payload cannot be decoded.
    let mut tampered = encoded;
    tampered["kind"]["unknown"]["amount"] = json!({
        "schema": "kiana.money.v1",
        "currency": "USD",
        "micros": 0
    });
    assert!(serde_json::from_value::<CostBreakdown>(tampered).is_err());
}

#[test]
fn receipt_breakdown_requires_unique_attempts_and_rejects_replayed_entries() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let orphan = CostBreakdown::unknown(
        estimate.run_id,
        None,
        digest('c'),
        UsageConfidence::Unknown,
        BillingUnknownReason::Partial,
    )
    .expect("orphan unknown");
    assert_eq!(
        ReceiptCostBreakdown::from_entries(
            vec![estimate.clone(), orphan],
            1,
            vec![EventId::new(), EventId::new()],
        )
        .unwrap_err(),
        "receipt_cost_attempt_missing"
    );

    let mut second_usage = usage.clone();
    second_usage.usage_id = UsageId::new();
    second_usage.attempt_id = estimate.attempt_id.expect("attempt");
    second_usage.usage_digest = second_usage.digest();
    let second =
        CostBreakdown::from_usage(&second_usage, Some(&card), 1, 2_000).expect("second estimate");
    assert_eq!(
        ReceiptCostBreakdown::from_entries(
            vec![estimate.clone(), second],
            1,
            vec![EventId::new(), EventId::new()],
        )
        .unwrap_err(),
        "receipt_cost_attempt_duplicate"
    );

    assert_eq!(
        ReceiptCostBreakdown::from_entries(
            vec![estimate.clone(), estimate],
            1,
            vec![EventId::new(), EventId::new()],
        )
        .unwrap_err(),
        "receipt_cost_entry_duplicate"
    );
}

#[test]
fn receipt_breakdown_recomputes_totals_and_rejects_tampered_totals() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let expected = estimate.kind.amount().expect("amount").clone();
    let mut receipt = ReceiptCostBreakdown::from_entries(vec![estimate], 1, vec![EventId::new()])
        .expect("receipt breakdown");
    assert_eq!(receipt.estimated_total.as_ref(), Some(&expected));
    assert_eq!(receipt.measured_total, None);
    assert!(receipt.unknown_reasons.is_empty());

    receipt.estimated_total = Some(Money::new("USD", expected.micros + 1).expect("money"));
    receipt.breakdown_digest = receipt.digest();
    assert_eq!(
        receipt.validate().unwrap_err(),
        "receipt_cost_breakdown_total_mismatch"
    );
}

#[test]
fn cost_reducer_refuses_duplicate_stream_gaps_kind_drift_and_usage_digest_drift() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let measured = CostBreakdown::from_provider_receipt(
        &usage,
        Some(&card),
        Money::new("USD", 29).expect("money"),
        ProviderReceiptRef::new("provider-receipt-reducer").expect("receipt"),
        2_000,
    )
    .expect("measured");
    let first = estimate
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let second = measured
        .to_runtime_event(RequestId::new(), 2)
        .expect("event");

    let mut gapped = second.clone();
    gapped.stream_version = Some(3);
    assert_eq!(
        project_receipt_cost_breakdown(usage.run_id, &[first.clone(), gapped]).unwrap_err(),
        "receipt_cost_event_revision_gap"
    );

    let mut duplicate = second.clone();
    duplicate.sequence = 3;
    duplicate.event_id = first.event_id;
    assert_eq!(
        project_receipt_cost_breakdown(usage.run_id, &[first.clone(), duplicate]).unwrap_err(),
        "receipt_cost_duplicate_source_event"
    );

    let mut drifted_kind = second.clone();
    drifted_kind.kind = COST_EVENT_ESTIMATED.to_owned();
    assert_eq!(
        project_receipt_cost_breakdown(usage.run_id, &[first.clone(), drifted_kind]).unwrap_err(),
        "receipt_cost_event_kind_mismatch"
    );

    let mut drifted_usage = usage.clone();
    drifted_usage.usage_id = UsageId::new();
    drifted_usage.usage_digest = drifted_usage.digest();
    let drifted_cost = CostBreakdown::from_provider_receipt(
        &drifted_usage,
        Some(&card),
        Money::new("USD", 29).expect("money"),
        ProviderReceiptRef::new("provider-receipt-drift").expect("receipt"),
        2_000,
    )
    .expect("drifted measured");
    let drifted_event = drifted_cost
        .to_runtime_event(RequestId::new(), 2)
        .expect("event");
    assert_eq!(
        project_receipt_cost_breakdown(usage.run_id, &[first.clone(), drifted_event]).unwrap_err(),
        "receipt_cost_event_transition_invalid"
    );

    let mut foreign_stream = second;
    foreign_stream.aggregate_id = Some(AttemptId::new().to_string());
    assert_eq!(
        project_receipt_cost_breakdown(usage.run_id, &[first, foreign_stream]).unwrap_err(),
        "receipt_cost_event_stream_mismatch"
    );
}

#[test]
fn estimate_and_measured_facts_are_registered_billing_attempt_events() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let measured = CostBreakdown::from_provider_receipt(
        &usage,
        Some(&card),
        Money::new("USD", 29).expect("money"),
        ProviderReceiptRef::new("provider-receipt-event").expect("receipt"),
        2_000,
    )
    .expect("measured");
    let unknown = CostBreakdown::unknown(
        usage.run_id,
        Some(AttemptId::new()),
        digest('e'),
        UsageConfidence::Unknown,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("unknown");

    for (fact, event_kind) in [
        (estimate.clone(), COST_EVENT_ESTIMATED),
        (measured.clone(), COST_EVENT_MEASURED),
        (unknown, COST_EVENT_UNKNOWN),
    ] {
        let runtime = fact.to_runtime_event(RequestId::new(), 1).expect("event");
        assert_eq!(runtime.kind, event_kind);
        assert_eq!(runtime.aggregate_type.as_deref(), Some("billing_attempt"));
        validate_runtime_event(&runtime).expect("registered cost fact");
        validate_event_payload(&runtime.kind, &runtime.data).expect("registered cost payload");
    }

    let estimate_event = estimate
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let mut missing_attempt = estimate_event.clone();
    missing_attempt
        .data
        .as_object_mut()
        .expect("object")
        .remove("attempt_id");
    assert_eq!(
        validate_event_payload(COST_EVENT_ESTIMATED, &missing_attempt.data).unwrap_err(),
        "event_required_id_missing:attempt_id"
    );

    let mut extra_field = estimate_event;
    extra_field.data["permit_id"] = json!("permit-1");
    assert_eq!(
        validate_event_payload(COST_EVENT_ESTIMATED, &extra_field.data).unwrap_err(),
        "event_payload_unknown_field"
    );
}
