//! EQ-46 Domain evaluator 的小型内存合同测试。
//!
//! 这些用例直接构造版本报告、阈值和摘要，不从外部读取当前策略或监控数据。它们检查阈值超限、样本不足、
//! route/grant 快照不一致、摘要篡改和严格 JSON 字段边界；不证明告警已持久化、已通知用户或影响实际路由。
use kiana_domain::{
    evaluate_drift, DriftAlertId, DriftEvaluationInput, DriftReport, DriftThreshold, DriftVerdict,
    EventId, RouteDecision, DRIFT_ALERT_EVENT_KIND, DRIFT_INPUT_SCHEMA,
};
use serde_json::json;

const TARGET: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROUTE: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const GRANT: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

/// 构造含指定 turn 数的单版本报告；`error=true` 表示每个合成样本都记为错误。
/// 通过 Domain `DriftReport::record()` 累加，避免手工篡改计数绕过源报告校验。
fn report(error: bool, turns: usize) -> DriftReport {
    let route = RouteDecision {
        schema: "kiana.route-decision.v1".to_owned(),
        provider_id: "fake".to_owned(),
        model_id: "model-a".to_owned(),
        model_profile: "default".to_owned(),
        prompt_hash: "prompt-v1".to_owned(),
        route_digest: "route-v1".to_owned(),
        configuration_revision: "config-v1".to_owned(),
        budget_schema: "budget-v1".to_owned(),
        runtime_version: "runtime-v1".to_owned(),
    };
    let mut report = DriftReport::default();
    for index in 0..turns {
        report
            .record(route.clone(), EventId::new(), 100 + index as u64, error)
            .unwrap();
    }
    report
}

/// 构造含错误率上限和固定 1 秒耗时上限的合同，并按字段重算 threshold digest。
fn threshold(minimum_samples: u64, max_error_rate_milli: Option<u64>) -> DriftThreshold {
    let mut threshold = DriftThreshold {
        schema: "kiana.quality-drift-threshold.v1".to_owned(),
        minimum_samples,
        max_error_rate_milli,
        max_mean_elapsed_ms: Some(1_000),
        threshold_digest: String::new(),
    };
    threshold.threshold_digest = threshold.digest();
    threshold
}

/// 组合合成 report/threshold；前后快照故意相等，source_cursor=9 仅满足非零形状要求。
fn make_input(report: DriftReport, threshold: DriftThreshold) -> DriftEvaluationInput {
    DriftEvaluationInput {
        schema: DRIFT_INPUT_SCHEMA.to_owned(),
        target_digest: TARGET.to_owned(),
        report,
        threshold,
        route_digest_before: ROUTE.to_owned(),
        route_digest_after: ROUTE.to_owned(),
        grant_digest_before: GRANT.to_owned(),
        grant_digest_after: GRANT.to_owned(),
        source_cursor: 9,
        alert_id: DriftAlertId::new(),
    }
}

#[test]
fn drift_threshold_breach_emits_alert_without_authority_change() {
    // 两个样本均失败，错误率为 1000 milli，严格大于 100 milli，因此生成告警证据。
    let result = evaluate_drift(&make_input(report(true, 2), threshold(1, Some(100)))).unwrap();
    assert_eq!(result.metrics.verdict, DriftVerdict::Alerted);
    let event = result.event.expect("threshold breach should emit an event");
    assert_eq!(event.kind, DRIFT_ALERT_EVENT_KIND);
    assert!(!event.alert.authority_changes_applied);
    assert_eq!(
        event.alert.route_digest_before,
        event.alert.route_digest_after
    );
    assert_eq!(
        event.alert.grant_digest_before,
        event.alert.grant_digest_after
    );
    assert!(event.validate().is_ok());
}

#[test]
fn insufficient_samples_need_review_and_do_not_pass() {
    // 单个样本达不到 minimum_samples=2；即使错误率没有严格超过 1000 milli，也不能报 Healthy。
    let result = evaluate_drift(&make_input(report(false, 1), threshold(2, Some(1_000)))).unwrap();
    assert_eq!(result.metrics.verdict, DriftVerdict::NeedsReview);
    assert!(result.event.is_none());
}

#[test]
fn route_or_grant_drift_is_rejected_before_alerting() {
    // 任一前后快照不同都在指标计算前拒绝，避免把配置变化的结果伪装为相同运行条件下的 drift。
    let mut input = make_input(report(true, 2), threshold(1, Some(100)));
    input.route_digest_after = TARGET.to_owned();
    assert_eq!(evaluate_drift(&input), Err("drift_input_invalid"));

    let mut input = make_input(report(true, 2), threshold(1, Some(100)));
    input.grant_digest_after = TARGET.to_owned();
    assert_eq!(evaluate_drift(&input), Err("drift_input_invalid"));
}

#[test]
fn drift_rejects_digest_and_unknown_field_tampering() {
    // JSON 仍可反序列化，但 threshold 摘要必须匹配阈值字段；额外字段由 deny_unknown_fields 拒绝。
    let mut input =
        serde_json::to_value(make_input(report(false, 2), threshold(1, Some(1_000)))).unwrap();
    input["threshold"]["threshold_digest"] = json!(TARGET);
    let tampered: DriftEvaluationInput = serde_json::from_value(input).unwrap();
    assert_eq!(evaluate_drift(&tampered), Err("drift_input_invalid"));

    let mut input =
        serde_json::to_value(make_input(report(false, 2), threshold(1, Some(1_000)))).unwrap();
    input["unexpected"] = json!(true);
    assert!(serde_json::from_value::<DriftEvaluationInput>(input).is_err());
}
