//! BQ-03 snapshot/delta/final 累计器的内存行为 fixture。
//!
//! 测试把多条合成 NormalizedUsage 交给同一个 attempt accumulator，观察累计值、sequence 去重/冲突、
//! 快照包含关系、溢出和 Final 后关闭。该 reducer 当前没有持久化或跨进程恢复测试。

use kiana_domain::{
    AttemptId, NormalizedUsage, RunId, UsageAccumulator, UsageApplyOutcome, UsageConfidence,
    UsageId, UsageObservation, UsageSource, UsageVector,
};

/// 在指定 attempt 内构造带 sequence 的合成观测。
///
/// 输入 token 字段使用调用者给出的 Option 以覆盖未知/零/正数；每次 helper 都生成新的 RunId，而 accumulator
/// 当前只按 attempt_id 检查边界，因此这些 fixture 不证明同一 attempt 的 run_id 一致性。
fn usage(
    attempt_id: AttemptId,
    observation: UsageObservation,
    sequence: u64,
    input: Option<u64>,
    output: Option<u64>,
) -> NormalizedUsage {
    let mut vector = UsageVector::zero();
    vector.input_tokens = input;
    vector.output_tokens = output;
    NormalizedUsage::new(
        UsageId::new(),
        attempt_id,
        RunId::new(),
        Some("provider:test".to_owned()),
        "model:test",
        "route:test",
        vector,
        UsageSource::Provider,
        observation,
        Some(sequence),
        UsageConfidence::Known,
        None,
        "fixture",
        format!(
            "sha256:{}",
            char::from(b'a' + (sequence as u8 % 20))
                .to_string()
                .repeat(64)
        ),
    )
    .unwrap()
}

#[test]
/// 按 cumulative Snapshot → Delta → Final 折叠，确认增量参与累计、Final 保存最终总值并关闭状态。
fn snapshot_delta_final_accumulate_with_sequence_dedup() {
    let attempt = AttemptId::new();
    let snapshot = usage(attempt, UsageObservation::Snapshot, 1, Some(10), Some(2));
    let delta = usage(attempt, UsageObservation::Delta, 2, Some(1), Some(0));
    let final_usage = usage(attempt, UsageObservation::Final, 3, Some(11), Some(2));
    let mut accumulator = UsageAccumulator::new(attempt).unwrap();
    assert_eq!(
        accumulator.apply(&snapshot).unwrap(),
        UsageApplyOutcome::Applied
    );
    assert_eq!(
        accumulator.apply(&delta).unwrap(),
        UsageApplyOutcome::Applied
    );
    assert_eq!(
        accumulator.apply(&final_usage).unwrap(),
        UsageApplyOutcome::Applied
    );
    assert!(accumulator.finalized());
    assert_eq!(accumulator.last_sequence(), Some(3));
    assert_eq!(accumulator.latest().unwrap().input_tokens, Some(11));
    assert_eq!(accumulator.latest().unwrap().output_tokens, Some(2));
}

#[test]
/// 同一序号同一 digest 返回 Duplicate；同序号不同内容冲突，低序号回退失败，下降快照不能覆盖已知值。
/// 所有错误都来自内存 reducer，不涉及 EventLog 的全局 sequence 校验。
fn sequence_duplicate_conflict_regression_and_containment_fail_closed() {
    let attempt = AttemptId::new();
    let snapshot = usage(attempt, UsageObservation::Snapshot, 1, Some(10), Some(2));
    let mut accumulator = UsageAccumulator::new(attempt).unwrap();
    accumulator.apply(&snapshot).unwrap();
    assert_eq!(
        accumulator.apply(&snapshot).unwrap(),
        UsageApplyOutcome::Duplicate
    );

    let conflict = usage(attempt, UsageObservation::Snapshot, 1, Some(11), Some(2));
    assert_eq!(
        accumulator.apply(&conflict).unwrap_err(),
        "usage_sequence_conflict"
    );
    let mut ordered = UsageAccumulator::new(attempt).unwrap();
    ordered
        .apply(&usage(
            attempt,
            UsageObservation::Snapshot,
            2,
            Some(10),
            Some(2),
        ))
        .unwrap();
    let regression = usage(attempt, UsageObservation::Snapshot, 1, Some(10), Some(2));
    assert_eq!(
        ordered.apply(&regression).unwrap_err(),
        "usage_sequence_regression"
    );

    let lower = usage(attempt, UsageObservation::Snapshot, 2, Some(9), Some(2));
    assert_eq!(
        accumulator.apply(&lower).unwrap_err(),
        "usage_snapshot_not_containing:input_tokens"
    );
}

#[test]
/// 验证 Delta 没有累计基线、checked addition 溢出和 Final 后新增观测三种拒绝。
/// 当前 fixture 用更大的新 sequence 测 post-final；没有验证完全相同的 Final 消息重投递行为。
fn unknown_delta_base_overflow_and_post_final_are_rejected() {
    let attempt = AttemptId::new();
    let delta = usage(attempt, UsageObservation::Delta, 1, Some(1), Some(0));
    assert_eq!(
        UsageAccumulator::new(attempt)
            .unwrap()
            .apply(&delta)
            .unwrap_err(),
        "usage_delta_without_snapshot"
    );

    let mut max = UsageVector::zero();
    max.input_tokens = Some(u64::MAX);
    let snapshot = NormalizedUsage::new(
        UsageId::new(),
        attempt,
        RunId::new(),
        None,
        "model",
        "route",
        max,
        UsageSource::Provider,
        UsageObservation::Snapshot,
        Some(1),
        UsageConfidence::Known,
        None,
        "fixture",
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    )
    .unwrap();
    let mut accumulator = UsageAccumulator::new(attempt).unwrap();
    accumulator.apply(&snapshot).unwrap();
    assert_eq!(
        accumulator
            .apply(&usage(
                attempt,
                UsageObservation::Delta,
                2,
                Some(1),
                Some(0)
            ))
            .unwrap_err(),
        "usage_accumulator_overflow:input_tokens"
    );

    let final_usage = usage(attempt, UsageObservation::Final, 3, Some(u64::MAX), Some(2));
    accumulator.apply(&final_usage).unwrap();
    assert_eq!(
        accumulator
            .apply(&usage(
                attempt,
                UsageObservation::Snapshot,
                4,
                Some(u64::MAX),
                Some(2)
            ))
            .unwrap_err(),
        "usage_accumulator_after_final"
    );
}
