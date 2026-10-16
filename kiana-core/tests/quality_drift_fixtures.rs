//! EQ-46 failure-first 合同 fixture：覆盖版本桶指标、告警和 `drift.alerted` 事件载荷。
//!
//! 每个测试先构造应被拒绝或审慎处理的输入，再断言稳定错误码或结构化 verdict。测试直接调用 Core 的
//! 公开薄适配器，但不经过产品命令路由，也不从真实数据源生成 report/threshold；所有摘要、游标、桶和
//! 计数都是内存合成值。这里不做线上监控读取、不追加事实事件、不切换配置、不调 provider，因此 fixture
//! 只能证明给定输入上的合同行为，不能证明后台告警已接通或真实数据来源可信。

use kiana_core::{evaluate_quality_drift, DRIFT_ALERT_COMMAND};
use kiana_domain::{
    DriftAlertEvent, DriftAlertId, DriftEvaluationInput, DriftReport, DriftThreshold, DriftVerdict,
    EventId, RouteDecision, DRIFT_ALERT_EVENT_KIND, DRIFT_ALERT_EVENT_SCHEMA, DRIFT_ALERT_SCHEMA,
    DRIFT_INPUT_SCHEMA, ROUTE_DECISION_SCHEMA,
};
use serde_json::json;

/// `DriftThreshold::validate` compares against this literal; it has no exported constant.
const THRESHOLD_SCHEMA: &str = "kiana.quality-drift-threshold.v1";

/// 构造测试用的摘要外形：固定 `sha256:` 前缀和 64 个重复十六进制字符。
/// 这满足格式校验，但不代表内容是实际计算出的哈希。
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

/// 构造测试用版本身份；字段仅用来产生不同的版本 key，不对应真实模型配置。
fn route(model_id: &str) -> RouteDecision {
    RouteDecision {
        schema: ROUTE_DECISION_SCHEMA.to_owned(),
        provider_id: "provider:fixture".to_owned(),
        model_id: model_id.to_owned(),
        model_profile: "default".to_owned(),
        prompt_hash: "prompt-v1".to_owned(),
        route_digest: "route-v1".to_owned(),
        configuration_revision: "config-v1".to_owned(),
        budget_schema: "budget-v1".to_owned(),
        runtime_version: "runtime-v1".to_owned(),
    }
}

/// 通过基础 DriftReport API 添加样本，避免 fixture 直接捏造桶内计数和 EventId 列表。
/// 前 `errors` 条样本记为失败，每条耗时都使用给定的 `mean_elapsed_ms`。
fn record_bucket(
    report: &mut DriftReport,
    model_id: &str,
    turns: u64,
    errors: u64,
    mean_elapsed_ms: u64,
) {
    for index in 0..turns {
        report
            .record(
                route(model_id),
                EventId::new(),
                mean_elapsed_ms,
                index < errors,
            )
            .expect("EQ-46 drift sample");
    }
}

/// 构造单个版本桶：4 次调用、每次 100 ms，因此错误率为 `errors * 250` milli。
fn single_bucket_report(errors: u64) -> DriftReport {
    let mut report = DriftReport::default();
    record_bucket(&mut report, "model-a", 4, errors, 100);
    report
}

/// 构造 threshold 并从其合同字段计算摘要，确保测试重点落在 evaluator 输入而非摘要装配错误。
fn threshold(
    minimum_samples: u64,
    max_error_rate_milli: Option<u64>,
    max_mean_elapsed_ms: Option<u64>,
) -> DriftThreshold {
    let mut threshold = DriftThreshold {
        schema: THRESHOLD_SCHEMA.to_owned(),
        minimum_samples,
        max_error_rate_milli,
        max_mean_elapsed_ms,
        threshold_digest: String::new(),
    };
    threshold.threshold_digest = threshold.digest();
    threshold
}

/// 把全部合成值组合成评估输入；游标 9 只是非零样例，route/grant 前后摘要刻意保持相同。
fn input(report: DriftReport, threshold: DriftThreshold) -> DriftEvaluationInput {
    DriftEvaluationInput {
        schema: DRIFT_INPUT_SCHEMA.to_owned(),
        target_digest: digest('a'),
        report,
        threshold,
        route_digest_before: digest('b'),
        route_digest_after: digest('b'),
        grant_digest_before: digest('c'),
        grant_digest_after: digest('c'),
        source_cursor: 9,
        alert_id: DriftAlertId::new(),
    }
}

/// 准备 4 个样本、其中 2 个失败的输入，错误率 500 milli，高于下方 400 milli 限值。
fn breached_input() -> DriftEvaluationInput {
    input(
        single_bucket_report(2),
        threshold(4, Some(400), Some(1_000)),
    )
}

/// route 前后摘要不一致时，在计算指标或产生告警前拒绝输入。
#[test]
fn route_digest_mismatch_is_rejected_before_any_alert() {
    let mut candidate = breached_input();
    candidate.route_digest_after = digest('e');
    assert_eq!(
        evaluate_quality_drift(&candidate),
        Err("drift_input_invalid")
    );
}

/// grant 前后摘要不一致时，同样 fail closed，不返回告警对象。
#[test]
fn grant_digest_mismatch_is_rejected_before_any_alert() {
    let mut candidate = breached_input();
    candidate.grant_digest_after = digest('e');
    assert_eq!(
        evaluate_quality_drift(&candidate),
        Err("drift_input_invalid")
    );
}

/// 把合法桶搬到错误 key 下，先验证 Domain 报告拒绝错配，再验证 Core adapter 将子合同失败
/// 折叠为 `drift_input_invalid`，不对错误归因的数据继续计算。
#[test]
fn version_bucket_key_mismatch_is_rejected() {
    let mut report = single_bucket_report(2);
    let original_key = report
        .buckets
        .keys()
        .next()
        .expect("EQ-46 single version bucket")
        .clone();
    let bucket = report
        .buckets
        .remove(&original_key)
        .expect("EQ-46 relocatable bucket");
    report.buckets.insert(digest('d'), bucket);

    assert_eq!(report.validate(), Err("drift_bucket_key_mismatch"));
    assert_eq!(
        evaluate_quality_drift(&input(report, threshold(4, Some(400), Some(1_000)))),
        Err("drift_input_invalid")
    );
}

/// source report 若声称自动切换标志为 true，会被 DriftReport validator 拒绝；
/// evaluator 本身没有选择或切换模型的职责。
#[test]
fn report_claiming_automatic_model_switch_is_rejected() {
    let mut report = single_bucket_report(2);
    report.automatic_model_switch = true;

    assert_eq!(report.validate(), Err("drift_report_invalid"));
    assert_eq!(
        evaluate_quality_drift(&input(report, threshold(4, Some(400), Some(1_000)))),
        Err("drift_input_invalid")
    );
}

/// 合同允许空 DriftReport，但 evaluator 必须用 `drift_samples_missing` 拒绝，不能静默给 Healthy。
#[test]
fn empty_report_is_rejected_without_samples() {
    let report = DriftReport::default();
    assert!(report.validate().is_ok());
    assert_eq!(
        evaluate_quality_drift(&input(report, threshold(4, Some(400), Some(1_000)))),
        Err("drift_samples_missing")
    );
}

/// minimum_samples=0 超出阈值合同；两个上限都为空也没有判定条件；二者都由 adapter 拒绝。
#[test]
fn invalid_or_empty_threshold_is_rejected() {
    let degenerate = threshold(0, Some(400), Some(1_000));
    assert_eq!(degenerate.validate(), Err("drift_threshold_invalid"));
    assert_eq!(
        evaluate_quality_drift(&input(single_bucket_report(2), degenerate)),
        Err("drift_input_invalid")
    );

    let limitless = threshold(4, None, None);
    assert_eq!(limitless.validate(), Err("drift_threshold_empty"));
    assert_eq!(
        evaluate_quality_drift(&input(single_bucket_report(2), limitless)),
        Err("drift_input_invalid")
    );
}

/// 样本量不足时返回 NeedsReview，且 alert/event 都为空；不足证据不升级为告警 payload。
#[test]
fn insufficient_samples_never_produce_an_alert() {
    let mut report = DriftReport::default();
    record_bucket(&mut report, "model-a", 1, 1, 100);
    let candidate = input(report, threshold(2, Some(400), Some(1_000)));

    let evaluation = evaluate_quality_drift(&candidate).expect("EQ-46 review verdict");
    assert_eq!(evaluation.metrics.verdict, DriftVerdict::NeedsReview);
    assert!(evaluation.alert.is_none());
    assert!(evaluation.event.is_none());
    assert!(evaluation.metrics.validate().is_ok());
}

/// 对可反序列化但内容被篡改的 payload 单独校验：authority flag、前后 route 摘要或事件 kind
/// 变化后，已有摘要不再匹配，嵌套告警/外层事件都会被拒绝。
#[test]
fn tampered_alert_or_event_payload_cannot_publish_itself() {
    let evaluation = evaluate_quality_drift(&breached_input()).expect("EQ-46 breach");
    let event = evaluation.event.expect("EQ-46 alert event");
    assert!(event.validate().is_ok());
    assert!(event.alert.validate().is_ok());

    let mut escalated = serde_json::to_value(&event).expect("EQ-46 event json");
    escalated["alert"]["authority_changes_applied"] = json!(true);
    let escalated: DriftAlertEvent = serde_json::from_value(escalated).expect("EQ-46 reserialized");
    assert_eq!(escalated.alert.validate(), Err("drift_alert_invalid"));
    assert_eq!(escalated.validate(), Err("drift_alert_event_invalid"));

    let mut rerouted = serde_json::to_value(&event).expect("EQ-46 event json");
    rerouted["alert"]["route_digest_after"] = json!(digest('e'));
    let rerouted: DriftAlertEvent = serde_json::from_value(rerouted).expect("EQ-46 reserialized");
    assert_eq!(rerouted.alert.validate(), Err("drift_alert_invalid"));
    assert_eq!(rerouted.validate(), Err("drift_alert_event_invalid"));

    let mut renamed = serde_json::to_value(&event).expect("EQ-46 event json");
    renamed["kind"] = json!("drift.remediated");
    let renamed: DriftAlertEvent = serde_json::from_value(renamed).expect("EQ-46 reserialized");
    assert_eq!(renamed.validate(), Err("drift_alert_event_invalid"));
}

/// 两版本输出按 key 严格排序；重复版本桶或交换顺序都会破坏指标合同。
/// 此处只改测试副本的 buckets，不影响 evaluator 原始结果。
#[test]
fn cross_version_bucket_duplicate_or_reordered_metrics_are_rejected() {
    let key_a = route("model-a").version_key();
    let key_b = route("model-b").version_key();
    assert_ne!(
        key_a, key_b,
        "distinct models must yield distinct version keys"
    );

    let mut report = DriftReport::default();
    record_bucket(&mut report, "model-a", 4, 0, 100);
    record_bucket(&mut report, "model-b", 4, 4, 100);
    let evaluation = evaluate_quality_drift(&input(report, threshold(4, Some(400), Some(1_000))))
        .expect("EQ-46 two-version evaluation");

    let metrics = evaluation.metrics.clone();
    assert_eq!(metrics.buckets.len(), 2);
    assert!(metrics.buckets[0].version_key < metrics.buckets[1].version_key);
    assert!(metrics.validate().is_ok());
    // Only the breaching version bucket alerts, whichever order the version keys hash into.
    assert_eq!(evaluation.alert.expect("EQ-46 alert").version_key, key_b);

    let mut duplicated = metrics.clone();
    duplicated.buckets.push(duplicated.buckets[0].clone());
    assert_eq!(duplicated.validate(), Err("drift_metrics_invalid"));

    let mut reordered = metrics;
    reordered.buckets.swap(0, 1);
    assert_eq!(reordered.validate(), Err("drift_metrics_invalid"));
}

/// 样本足够且错误率/均耗时都未严格超过限值时为 Healthy，不产生 alert/event。
#[test]
fn below_threshold_metrics_stay_healthy_and_emit_no_alert() {
    let candidate = input(
        single_bucket_report(0),
        threshold(4, Some(400), Some(1_000)),
    );
    assert!(!candidate.report.automatic_model_switch);

    let evaluation = evaluate_quality_drift(&candidate).expect("EQ-46 healthy verdict");
    assert_eq!(evaluation.metrics.verdict, DriftVerdict::Healthy);
    assert!(evaluation.alert.is_none());
    assert!(evaluation.event.is_none());
    assert!(evaluation.metrics.validate().is_ok());
    assert_eq!(evaluation.metrics.buckets.len(), 1);
    assert_eq!(evaluation.metrics.buckets[0].error_rate_milli, 0);
    assert_eq!(evaluation.metrics.buckets[0].mean_elapsed_ms, 100);
}

/// 主验收 fixture：超限桶生成结构完整的告警事件，同时输入 route/grant 前后摘要不变，
/// authority flag 保持 false，且 report 不携带自动切换声明。所有载荷各自通过合同校验，
/// 但这仍只证明内存计算结果，不证明事件已持久化。
#[test]
fn drift_alert_does_not_change_route_or_grant() {
    let candidate = breached_input();
    let evaluation = evaluate_quality_drift(&candidate).expect("EQ-46 breach evaluation");

    assert_eq!(evaluation.metrics.verdict, DriftVerdict::Alerted);
    assert!(evaluation.metrics.validate().is_ok());

    let alert = evaluation.alert.expect("EQ-46 alert evidence");
    assert_eq!(alert.schema, DRIFT_ALERT_SCHEMA);
    assert!(!alert.authority_changes_applied);
    assert_eq!(alert.route_digest_before, digest('b'));
    assert_eq!(alert.route_digest_after, alert.route_digest_before);
    assert_eq!(alert.grant_digest_before, digest('c'));
    assert_eq!(alert.grant_digest_after, alert.grant_digest_before);
    assert_eq!(alert.reason, "drift_threshold_exceeded");
    assert_eq!(alert.version_key, route("model-a").version_key());
    assert_eq!(alert.sample_count, 4);
    assert!(alert.validate().is_ok());

    let event = evaluation.event.expect("EQ-46 drift.alerted event");
    assert_eq!(event.schema, DRIFT_ALERT_EVENT_SCHEMA);
    assert_eq!(event.kind, DRIFT_ALERT_EVENT_KIND);
    assert_eq!(event.kind, DRIFT_ALERT_COMMAND);
    assert_eq!(event.source_cursor, 9);
    assert_eq!(event.alert, alert);
    assert!(event.validate().is_ok());

    assert!(!candidate.report.automatic_model_switch);
}
