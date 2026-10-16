//! BQ-03 的源码边界守卫。
//!
//! 该测试只读取 accumulator 源码字符串并确认状态类型、错误名和三种 observation 分支仍在文本中；它不会
//! 应用一条 usage，也不能证明 reducer 已被接入 attempt 的持久事件流。

#[test]
/// 锚定 sequence 幂等/冲突、单调快照、Delta 基线、溢出和 Final 分支的源码标记。
fn usage_accumulator_guards_sequence_dedup_containment_and_overflow() {
    let accumulator = include_str!("../../kiana-domain/src/billing_usage_accumulator.rs");
    for marker in [
        "UsageAccumulator",
        "UsageApplyOutcome",
        "usage_sequence_conflict",
        "usage_sequence_regression",
        "usage_snapshot_not_containing",
        "usage_delta_without_snapshot",
        "usage_accumulator_overflow",
        "UsageObservation::Snapshot",
        "UsageObservation::Delta",
        "UsageObservation::Final",
    ] {
        assert!(
            accumulator.contains(marker),
            "accumulator marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest",
        "tokio",
        "std::fs",
        "CapabilityBroker",
        "SystemTime",
    ] {
        assert!(
            !accumulator.contains(forbidden),
            "accumulator boundary widened: {forbidden}"
        );
    }
}
