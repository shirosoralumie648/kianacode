use kiana_domain::{
    AttemptId, NormalizedUsage, RunId, UsageAccumulator, UsageApplyOutcome, UsageConfidence,
    UsageId, UsageObservation, UsageSource, UsageVector,
};

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
