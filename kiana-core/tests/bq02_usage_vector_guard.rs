//! BQ-02 的源码边界守卫。
//!
//! include_str! 将 Domain 文件和 roadmap baseline 当作文本读取；marker 与 forbidden 检查只约束合同字段和
//! 依赖边界，不运行归一化、不校验真实 provider 回包，也不证明计量来源已认证。

#[test]
/// 检查 UsageVector/NormalizedUsage 的关键字段仍存在，且 Domain 值对象没有引入 I/O/运行时依赖。
fn usage_vector_contract_keeps_unknown_zero_and_provenance_distinct() {
    let usage = include_str!("../../kiana-domain/src/billing_usage.rs");
    let baseline = include_str!("../../docs/roadmap/billing-contract-baseline.md");
    for marker in [
        "UsageVector",
        "NormalizedUsage",
        "UsagePresence",
        "ExplicitZero",
        "UsageConfidence",
        "UsageSource",
        "UsageObservation",
        "sequence",
        "unknown_reason",
        "raw_digest",
        "USAGE_VECTOR_SCHEMA",
        "NORMALIZED_USAGE_SCHEMA",
    ] {
        assert!(usage.contains(marker), "usage marker missing: {marker}");
    }
    assert!(baseline.contains("UsageVector"));
    assert!(baseline.contains("None` is unknown, not zero"));
    for forbidden in [
        "reqwest",
        "tokio",
        "std::fs",
        "CapabilityBroker",
        "RateCardStore",
    ] {
        assert!(
            !usage.contains(forbidden),
            "usage contract boundary widened: {forbidden}"
        );
    }
}
