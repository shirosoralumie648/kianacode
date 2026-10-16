//! EQ-46 只读与 authority 边界的源码 marker 守卫。
//!
//! 它用 `include_str!` 检查 Domain/Core 源码中存在指标/告警合同，并且没有列出的执行或变更标记。
//! 这是文本级防回归检查：它不调用 reducer、不验证实际事件写入路径，也不能证明其它模块运行时不可能触发 effect。
#[test]
fn drift_alert_is_evidence_only_and_keeps_route_and_grant_fenced() {
    // 守卫只读取这两个实现文件；其它文档和 fixture 中的同名字段不参与断言。
    let domain = include_str!("../../kiana-domain/src/quality_drift.rs");
    let core = include_str!("../src/quality_drift.rs");
    // 正向 marker 锁定公开合同名与前后快照字段仍在实现中。
    for marker in [
        "DriftThreshold",
        "DriftMetricBucket",
        "DriftMetrics",
        "DriftAlert",
        "DriftAlertEvent",
        "DRIFT_ALERT_EVENT_KIND",
        "minimum_samples",
        "authority_changes_applied",
        "route_digest_before",
        "grant_digest_before",
        "evaluate_quality_drift",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "EQ-46 marker missing: {marker}"
        );
    }
    // 黑名单扫描源文件的完整文本，注释也会被扫描，因此实现注释需避免复述这些字面标记。
    for forbidden in [
        "CapabilityBroker",
        "ModelClient",
        "std::process::Command",
        "EventStore",
        "route.switch",
        "grant.update",
        "automatic_model_switch = true",
        "authority_changes_applied: true",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "EQ-46 route/grant/effect marker present: {forbidden}"
        );
    }
}
