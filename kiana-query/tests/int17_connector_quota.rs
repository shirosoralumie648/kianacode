//! INT-17 Query quota projector 的静态源码边界守卫。
//!
//! `include_str!` 将 projector 源码作为文本检查事件种类、投影字段与只读边界 marker；本文件
//! 不构造 `RuntimeEvent`、不执行状态重放，也不验证产品路由当前是否调用该函数。

#[test]
fn quota_projection_is_replayable_and_never_an_authorizer() {
    let source = include_str!("../src/connector_quota.rs");
    // 这些 marker 证明状态种类和投影字段仍留在源码中，不证明事件时序处理正确。
    for marker in [
        "project_connector_quota",
        "ConnectorQuotaProjection",
        "CONNECTOR_QUOTA_EVENT_RESERVED",
        "CONNECTOR_QUOTA_EVENT_CLAIMED",
        "CONNECTOR_QUOTA_EVENT_SETTLED",
        "CONNECTOR_QUOTA_EVENT_RELEASED",
        "replayable",
        "projection_is_not_authorization",
        "source_cursor",
    ] {
        assert!(
            source.contains(marker),
            "INT-17 query marker missing: {marker}"
        );
    }
    // 禁止词约束只搜索此单一源码文件；它不是整个 workspace 的权限或依赖审计。
    for forbidden in [
        "CapabilityBroker",
        "authorize_and_execute",
        "resolve_secret",
        "ConnectorQuotaLedger",
    ] {
        assert!(
            !source.contains(forbidden),
            "quota query must stay read-only: {forbidden}"
        );
    }
}
