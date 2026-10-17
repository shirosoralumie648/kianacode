//! BQ-13 Query 成本投影的合成事件 fixture 与源码边界检查。
//!
//! 行为用例构造本地 rate card、usage 和 cost event，检查投影游标及估算/实测/未知区分；最后一个
//! 用例用 `include_str!` 检查 Query 源文件的只读依赖边界。所有用例均不调用真实 provider 或 EventStore。

use kiana_domain::*;
use kiana_query::{project_cost_receipt, CostQueryProjectionError};

/// 生成仅符合 sha256 文本格式的 fixture 值；不会执行真实 hash。
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

/// 构造仅供测试估算使用的临时费率卡。
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

/// 构造带有已知输入/输出 token 数的 Provider usage 事实。
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

/// 估算事件进入 Query wrapper 后保留 Domain cursor，并仍归类为 estimate。
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

/// 没有成本源事件必须返回不可用错误，不能把空输入伪装成零成本。
#[test]
fn query_projector_does_not_turn_empty_source_into_zero_cost() {
    let error = project_cost_receipt(RunId::new(), &[]).unwrap_err();
    assert_eq!(error, CostQueryProjectionError::SourceEmpty);
}

/// 静态检查 Query projector 仍委托 Domain reducer，且没有引入写入或 provider 依赖。
///
/// 该测试只做源文本 marker/禁止词断言，不运行 Domain reducer，也不证明整个 workspace 无其他执行路径。
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

/// 信封 cursor 必须与 Domain breakdown cursor 相等，漂移的 envelope 一律拒绝。
#[test]
fn query_projector_rejects_envelope_cursor_drift() {
    let usage = usage();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card()), 1, 2_000).expect("estimate");
    let event = estimate
        .to_runtime_event(RequestId::new(), 4)
        .expect("event");
    let mut projection = project_cost_receipt(usage.run_id, &[event]).expect("projection");
    projection.source_cursor += 1;
    assert_eq!(
        projection.validate().unwrap_err(),
        CostQueryProjectionError::Invalid("cost_query_projection_header_invalid".to_owned())
    );

    let event = estimate
        .to_runtime_event(RequestId::new(), 4)
        .expect("event");
    let mut projection = project_cost_receipt(usage.run_id, &[event]).expect("projection");
    projection.breakdown.source_cursor += 3;
    projection.breakdown.breakdown_digest = projection.breakdown.digest();
    assert_eq!(
        projection.validate().unwrap_err(),
        CostQueryProjectionError::Invalid("cost_query_projection_header_invalid".to_owned())
    );
}

/// Unknown 显示原因且没有金额；measured 只带 provider receipt 的金额。
#[test]
fn query_projector_keeps_unknown_reasons_and_measured_total_separate() {
    let usage = usage();
    let unknown = CostBreakdown::unknown(
        usage.run_id,
        Some(AttemptId::new()),
        digest('b'),
        UsageConfidence::Unknown,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("unknown");
    let event = unknown
        .to_runtime_event(RequestId::new(), 2)
        .expect("event");
    let projection = project_cost_receipt(usage.run_id, &[event]).expect("projection");
    assert_eq!(projection.breakdown.estimated_total, None);
    assert_eq!(projection.breakdown.measured_total, None);
    assert_eq!(
        projection.unknown_reasons(),
        &[BillingUnknownReason::ResultUnknown]
    );

    let measured = CostBreakdown::from_provider_receipt(
        &usage,
        Some(&card()),
        Money::new("USD", 7).expect("money"),
        ProviderReceiptRef::new("query-receipt-measured").expect("receipt"),
        2_000,
    )
    .expect("measured");
    let event = measured
        .to_runtime_event(RequestId::new(), 3)
        .expect("event");
    let projection = project_cost_receipt(usage.run_id, &[event]).expect("projection");
    assert_eq!(
        projection
            .breakdown
            .measured_total
            .as_ref()
            .map(|money| money.micros),
        Some(7)
    );
    assert_eq!(projection.breakdown.estimated_total, None);
    assert!(projection.unknown_reasons().is_empty());
}

/// 重复事件与解码失败的 payload 在 Query 层同样 fail-closed。
#[test]
fn query_projector_rejects_duplicate_and_malformed_cost_facts() {
    let usage = usage();
    let estimate = CostBreakdown::from_usage(&usage, Some(&card()), 1, 2_000).expect("estimate");
    let event = estimate
        .to_runtime_event(RequestId::new(), 4)
        .expect("event");
    let mut duplicate = event.clone();
    duplicate.sequence = 5;
    assert_eq!(
        project_cost_receipt(usage.run_id, &[event, duplicate]).unwrap_err(),
        CostQueryProjectionError::Invalid("receipt_cost_duplicate_source_event".to_owned())
    );

    let malformed = RuntimeEvent::new(
        RequestId::new(),
        1,
        "usage.cost_measured",
        serde_json::json!({"run_id": usage.run_id.to_string()}),
    )
    .expect("event");
    assert_eq!(
        project_cost_receipt(usage.run_id, &[malformed]).unwrap_err(),
        CostQueryProjectionError::Invalid("receipt_cost_event_decode_failed".to_owned())
    );
}
