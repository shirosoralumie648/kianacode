//! P4-L3-01 的 Domain 内存 fixture。
//!
//! 测试直接构造 JSON 元数据和 EventId，检查 RouteDecision 版本维度、DriftReport 分桶/计数、严格
//! Serde 往返以及禁止自动切换的不变量。它不启动 ControlPlane、不读取 EventStore、不调用 Provider，
//! 也不证明生产 `run.model_turn` 元数据完整或可信；真实查询入口见 `kiana-core/src/versioning.rs`。
use kiana_domain::{DriftReport, RouteDecision, DRIFT_REPORT_SCHEMA};
use serde_json::json;

#[test]
fn drift_report_is_bucketed_by_version() {
    // 模拟审计字段；budget.schema 是分类标签，不是 token 数或货币成本。
    let base = json!({
        "provider_id": "fake",
        "model_id": "fake-coder",
        "model_profile": "builder",
        "prompt_hash": "sha256:prompt-v1",
        "route_digest": "sha256:route-v1",
        "configuration_revision": "config-v1",
        "budget": {"schema": "kiana.runtime-budget.v1"}
    });
    let mut changed = base.clone();
    changed["model_profile"] = json!("reviewer");

    // 只修改 profile 就应该产生不同版本 key，避免把不同调用配置混到同一指标桶。
    let first = RouteDecision::from_model_turn(&base, "0.1.0");
    let second = RouteDecision::from_model_turn(&changed, "0.1.0");
    assert_ne!(first.version_key(), second.version_key());
    assert_eq!(first.schema, "kiana.route-decision.v1");
    first.validate().expect("route identity validates");

    let mut report = DriftReport::default();
    let first_event = kiana_domain::EventId::new();
    let second_event = kiana_domain::EventId::new();
    // 相同版本的两个事件共用桶并累加错误计数；profile 改变后再建立新桶。
    report
        .record(first.clone(), first_event, 12, false)
        .expect("first turn records");
    report
        .record(first, second_event, 8, true)
        .expect("same version shares a bucket");
    report
        .record(second, kiana_domain::EventId::new(), 4, false)
        .expect("profile change creates a new bucket");
    report.validate().expect("report validates");

    assert_eq!(report.schema, DRIFT_REPORT_SCHEMA);
    assert_eq!(report.buckets.len(), 2);
    assert_eq!(
        report
            .buckets
            .values()
            .map(|bucket| bucket.turns)
            .sum::<u64>(),
        3
    );
    assert_eq!(
        report
            .buckets
            .values()
            .map(|bucket| bucket.errors)
            .sum::<u64>(),
        1
    );

    // 这里只检查结构体 JSON 边界和拒绝未知字段，不涉及 EventStore 或协议 envelope。
    let encoded = serde_json::to_value(&report).expect("strict report serializes");
    let decoded: DriftReport = serde_json::from_value(encoded.clone()).expect("round trip");
    assert_eq!(decoded, report);
    let mut unknown = encoded;
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<DriftReport>(unknown).is_err());

    let mut automatic = report;
    automatic.automatic_model_switch = true;
    assert_eq!(automatic.validate(), Err("drift_report_invalid"));
}
