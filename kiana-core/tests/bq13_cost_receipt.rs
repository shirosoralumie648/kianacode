use kiana_core::aggregate_receipt_facts;
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
        4,
        "core-bq13-fixture",
    )
    .expect("card");
    card.request_price = Some(5);
    card.rate_card_digest = card.digest();
    card
}

fn usage() -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        Some("provider:test".to_owned()),
        "model:test",
        "route:test",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(2),
            output_tokens: Some(3),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        UsageConfidence::Known,
        None,
        "core fixture",
        digest('a'),
    )
    .expect("usage")
}

fn run_event(run_id: RunId, sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data)
        .expect("event")
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

#[test]
fn aggregate_receipt_exposes_cost_breakdown_and_unknown_reasons() {
    let usage = usage();
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let unknown = CostBreakdown::unknown(
        usage.run_id,
        Some(AttemptId::new()),
        digest('b'),
        UsageConfidence::Unknown,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("unknown");
    let estimated_event = estimate
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let unknown_event = unknown
        .to_runtime_event(RequestId::new(), 2)
        .expect("event");
    let aggregation = aggregate_receipt_facts(usage.run_id, &[estimated_event, unknown_event])
        .expect("aggregation");
    let breakdown = aggregation.cost_breakdown.as_ref().expect("cost breakdown");
    assert!(breakdown.estimated_total.is_some());
    assert_eq!(
        breakdown.unknown_reasons,
        vec![BillingUnknownReason::ResultUnknown]
    );
    assert!(aggregation.cost_micros.is_none());
    assert!(!aggregation.cost_estimated);
    assert!(aggregation.to_json().expect("json")["cost_breakdown"]["entries"].is_array());
}

#[test]
fn aggregate_rejects_cost_event_run_or_stream_drift() {
    let usage = usage();
    let card = card();
    let cost = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let mut foreign_usage = usage.clone();
    foreign_usage.run_id = RunId::new();
    foreign_usage.usage_digest = foreign_usage.digest();
    let foreign =
        CostBreakdown::from_usage(&foreign_usage, Some(&card), 1, 2_000).expect("foreign estimate");
    let event = foreign
        .to_runtime_event(RequestId::new(), 1)
        .expect("valid foreign event");
    assert_eq!(
        project_receipt_cost_breakdown(usage.run_id, &[event]).unwrap_err(),
        "receipt_cost_event_run_mismatch"
    );
    let mut event = cost.to_runtime_event(RequestId::new(), 1).expect("event");
    event.aggregate_id = Some(RunId::new().to_string());
    assert_eq!(
        project_receipt_cost_breakdown(usage.run_id, &[event]).unwrap_err(),
        "receipt_cost_event_stream_mismatch"
    );
}

#[test]
fn receipts_cost_boundary_is_projection_only() {
    let receipts = include_str!("../src/receipts.rs");
    let domain = include_str!("../../kiana-domain/src/billing_cost.rs");
    for marker in [
        "project_receipt_cost_breakdown",
        "cost_breakdown",
        "estimated_total",
        "measured_total",
        "unknown_reasons",
        "provider_receipt",
        "rate_card_version",
    ] {
        assert!(
            receipts.contains(marker) || domain.contains(marker),
            "BQ-13 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBrokerPort",
        "commit_transition",
        "FinancialBudget",
        "provider_estimate_settle",
        "cost = 0",
    ] {
        assert!(
            !domain.contains(forbidden),
            "cost projection must not {forbidden}"
        );
    }
}

#[test]
fn receipt_aggregation_keeps_legacy_cost_fields_in_sync_with_the_breakdown() {
    let usage = usage();
    let card = card();

    // Estimate only: the legacy fields mirror the typed estimated total.
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let estimated_amount = estimate.kind.amount().expect("amount").clone();
    let estimated_event = estimate
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let aggregation = aggregate_receipt_facts(usage.run_id, &[estimated_event]).expect("aggregate");
    assert_eq!(
        aggregation.cost_micros,
        u64::try_from(estimated_amount.micros).ok()
    );
    assert!(aggregation.cost_estimated);
    let breakdown = aggregation.cost_breakdown.as_ref().expect("breakdown");
    assert_eq!(breakdown.estimated_total.as_ref(), Some(&estimated_amount));
    assert_eq!(breakdown.measured_total, None);

    // Measured only: the legacy field is the measured amount and never flagged as an estimate.
    let measured = CostBreakdown::from_provider_receipt(
        &usage,
        Some(&card),
        Money::new("USD", 19).expect("money"),
        ProviderReceiptRef::new("provider-receipt-sync").expect("receipt"),
        2_000,
    )
    .expect("measured");
    let measured_event = measured
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let aggregation = aggregate_receipt_facts(usage.run_id, &[measured_event]).expect("aggregate");
    assert_eq!(aggregation.cost_micros, Some(19));
    assert!(!aggregation.cost_estimated);
    let breakdown = aggregation.cost_breakdown.as_ref().expect("breakdown");
    assert_eq!(
        breakdown.measured_total.as_ref().map(|money| money.micros),
        Some(19)
    );
    assert_eq!(breakdown.estimated_total, None);

    // Unknown cost keeps the residual unknown visible and never becomes a numeric zero.
    let unknown = CostBreakdown::unknown(
        usage.run_id,
        Some(AttemptId::new()),
        digest('b'),
        UsageConfidence::Unknown,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("unknown");
    let unknown_event = unknown
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let aggregation = aggregate_receipt_facts(usage.run_id, &[unknown_event]).expect("aggregate");
    assert_eq!(aggregation.cost_micros, None);
    assert!(!aggregation.cost_estimated);
    let breakdown = aggregation.cost_breakdown.as_ref().expect("breakdown");
    assert_eq!(
        breakdown.unknown_reasons,
        vec![BillingUnknownReason::ResultUnknown]
    );

    // Estimated and measured attempts stay in separate totals; the legacy single value is absent.
    let mut second_usage = usage.clone();
    second_usage.usage_id = UsageId::new();
    second_usage.attempt_id = AttemptId::new();
    second_usage.usage_digest = second_usage.digest();
    let second_measured = CostBreakdown::from_provider_receipt(
        &second_usage,
        Some(&card),
        Money::new("USD", 19).expect("money"),
        ProviderReceiptRef::new("provider-receipt-second-attempt").expect("receipt"),
        2_000,
    )
    .expect("measured");
    let estimate_event = estimate
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let second_event = second_measured
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let aggregation =
        aggregate_receipt_facts(usage.run_id, &[estimate_event, second_event]).expect("aggregate");
    assert_eq!(aggregation.cost_micros, None);
    assert!(!aggregation.cost_estimated);
    let breakdown = aggregation.cost_breakdown.as_ref().expect("breakdown");
    assert!(breakdown.estimated_total.is_some());
    assert_eq!(
        breakdown.measured_total.as_ref().map(|money| money.micros),
        Some(19)
    );
}

#[test]
fn receipt_aggregation_rejects_tampered_cost_consistency() {
    let usage = usage();
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let estimated_event = estimate
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let aggregation = aggregate_receipt_facts(usage.run_id, &[estimated_event]).expect("aggregate");

    // A recomputed self-digest cannot make the legacy flat field disagree with the breakdown.
    let mut decoded: ReceiptAggregation =
        serde_json::from_value(aggregation.to_json().expect("json")).expect("decode");
    decoded.cost_micros = Some(decoded.cost_micros.expect("legacy amount") + 1);
    decoded.aggregation_digest = decoded.digest();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "receipt_cost_breakdown_legacy_mismatch"
    );

    // A recomputed breakdown digest cannot hide a drifted total.
    let mut decoded: ReceiptAggregation =
        serde_json::from_value(aggregation.to_json().expect("json")).expect("decode");
    {
        let breakdown = decoded.cost_breakdown.as_mut().expect("breakdown");
        let total = breakdown.estimated_total.as_mut().expect("total");
        total.micros += 1;
        breakdown.breakdown_digest = breakdown.digest();
    }
    decoded.aggregation_digest = decoded.digest();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "receipt_cost_breakdown_total_mismatch"
    );

    // Unknown reasons must stay visible; a numeric legacy zero cannot be written next to them.
    let unknown = CostBreakdown::unknown(
        usage.run_id,
        Some(AttemptId::new()),
        digest('b'),
        UsageConfidence::Unknown,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("unknown");
    let unknown_event = unknown
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let aggregation = aggregate_receipt_facts(usage.run_id, &[unknown_event]).expect("aggregate");
    let mut decoded: ReceiptAggregation =
        serde_json::from_value(aggregation.to_json().expect("json")).expect("decode");
    decoded.cost_micros = Some(0);
    decoded.aggregation_digest = decoded.digest();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "receipt_cost_unknown_must_remain_visible"
    );
}

#[test]
fn aggregate_receipt_facts_rejects_duplicate_and_malformed_cost_facts() {
    let usage = usage();
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let event = estimate
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let mut duplicate = event.clone();
    duplicate.sequence = 2;
    assert_eq!(
        aggregate_receipt_facts(usage.run_id, &[event, duplicate]).unwrap_err(),
        "receipt_cost_projection:receipt_cost_duplicate_source_event"
    );

    let malformed = RuntimeEvent::new(
        RequestId::new(),
        1,
        "usage.cost_measured",
        json!({"run_id": usage.run_id.to_string()}),
    )
    .expect("event");
    assert_eq!(
        aggregate_receipt_facts(usage.run_id, &[malformed]).unwrap_err(),
        "receipt_cost_projection:receipt_cost_event_decode_failed"
    );
}
