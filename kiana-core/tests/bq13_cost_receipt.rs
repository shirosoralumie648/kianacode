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
    let breakdown = aggregation.cost_breakdown.expect("cost breakdown");
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
    let mut event = cost.to_runtime_event(RequestId::new(), 1).expect("event");
    event.data["run_id"] = json!(RunId::new());
    assert_eq!(
        project_receipt_cost_breakdown(usage.run_id, &[event]).unwrap_err(),
        "receipt_cost_event_run_mismatch"
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
