use kiana_domain::*;
use kiana_query::{project_cost_receipt, CostQueryProjectionError};

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn card() -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider:test",
        "model:test",
        "USD",
        Some(1),
        Some(2),
        1_000,
        Some(10_000),
        3,
        "query-bq13-fixture",
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
            input_tokens: Some(4),
            output_tokens: Some(2),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        UsageConfidence::Known,
        None,
        "query fixture",
        digest('a'),
    )
    .expect("usage")
}

#[test]
fn query_projector_preserves_cursor_and_measured_unknown_split() {
    let usage = usage();
    let card = card();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let event = estimate
        .to_runtime_event(RequestId::new(), 4)
        .expect("event");
    let projection = project_cost_receipt(usage.run_id, &[event]).expect("projection");
    assert_eq!(projection.source_cursor, 4);
    assert!(projection.breakdown.estimated_total.is_some());
    assert!(projection.breakdown.measured_total.is_none());
    assert!(projection.unknown_reasons().is_empty());
    projection.validate().expect("valid projection");
}

#[test]
fn query_projector_does_not_turn_empty_source_into_zero_cost() {
    let error = project_cost_receipt(RunId::new(), &[]).unwrap_err();
    assert_eq!(error, CostQueryProjectionError::SourceEmpty);
}

#[test]
fn query_projector_source_guard_is_read_only() {
    let source = include_str!("../src/cost_projector.rs");
    for marker in [
        "project_receipt_cost_breakdown",
        "source_cursor",
        "unknown_reasons",
        "ReceiptCostBreakdown",
        "CostQueryProjectionError",
    ] {
        assert!(source.contains(marker), "query marker missing: {marker}");
    }
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "tokio::spawn",
        "reqwest",
        "FinancialBudget",
        "release_reservation",
    ] {
        assert!(
            !source.contains(forbidden),
            "query boundary widened: {forbidden}"
        );
    }
}
