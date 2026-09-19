use kiana_domain::*;
use serde_json::json;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn binding(mode: HarnessReplayMode) -> HarnessTraceBinding {
    HarnessTraceBinding::new(
        GoldenTraceId::new(),
        digest('a'),
        "source:commit-1",
        "model:v1",
        "prompt:v3",
        digest('b'),
        digest('c'),
        digest('d'),
        "runtime:v1",
        4,
        Some(digest('e')),
        mode,
    )
    .unwrap()
}

#[test]
fn trace_binding_covers_source_model_prompt_tools_input_and_workspace() {
    let binding = binding(HarnessReplayMode::OfflineNoEffects);
    binding.validate().unwrap();
    let encoded = serde_json::to_value(&binding).unwrap();
    for field in [
        "source_snapshot",
        "model_version",
        "prompt_version",
        "tool_directory_hash",
        "input_hash",
        "workspace_hash",
    ] {
        assert!(
            encoded.get(field).is_some(),
            "missing binding field {field}"
        );
    }
    assert_eq!(
        schema_contract(HARNESS_TRACE_BINDING_SCHEMA)
            .unwrap()
            .owner_crate,
        "kiana-domain"
    );
}

#[test]
fn offline_replay_fails_missing_result_and_blocks_any_effect() {
    let binding = binding(HarnessReplayMode::OfflineNoEffects);
    let missing = HarnessReplayReport::new(
        &binding,
        5,
        digest('f'),
        0,
        0,
        0,
        true,
        false,
        Some(HarnessReplayDifference {
            index: 5,
            kind: HarnessReplayDifferenceKind::MissingResult,
            expected: Some("run.completed".to_owned()),
            observed: None,
            reason: "terminal result absent".to_owned(),
        }),
    )
    .unwrap();
    assert_eq!(missing.verdict, EvalVerdict::Fail);

    let effect = HarnessReplayReport::new(
        &binding,
        5,
        digest('f'),
        1,
        0,
        0,
        false,
        true,
        Some(HarnessReplayDifference {
            index: 3,
            kind: HarnessReplayDifferenceKind::ExtraSideEffect,
            expected: None,
            observed: Some("capability.completed".to_owned()),
            reason: "replay attempted an effect".to_owned(),
        }),
    )
    .unwrap();
    assert_eq!(effect.verdict, EvalVerdict::Blocked);

    let live = HarnessReplayReport::new(
        &binding(HarnessReplayMode::LiveProviderNotEvaluated),
        5,
        digest('f'),
        0,
        0,
        0,
        false,
        false,
        None,
    )
    .unwrap();
    assert_eq!(live.verdict, EvalVerdict::Blocked);
}

#[test]
fn metric_set_separates_incomplete_usage_and_bounds_cost_promotion() {
    let incomplete_cost = HarnessEvalMetricObservation::new(
        HarnessEvalMetric::RequestCost,
        HarnessEvalMetricStatus::NotMeasured,
        0,
        None,
        1,
        false,
        Some("provider usage incomplete".to_owned()),
    )
    .unwrap();
    let task = HarnessEvalMetricObservation::new(
        HarnessEvalMetric::TaskCompletion,
        HarnessEvalMetricStatus::Pass,
        1,
        Some(1),
        1,
        false,
        None,
    )
    .unwrap();
    let set = HarnessEvalMetricSet::new(
        digest('a'),
        "case-1",
        vec![task, incomplete_cost],
        false,
        vec!["usage_incomplete".to_owned()],
    )
    .unwrap();
    assert!(!set.promote);
    set.validate().unwrap();

    assert!(HarnessEvalMetricObservation::new(
        HarnessEvalMetric::RequestCost,
        HarnessEvalMetricStatus::Pass,
        1,
        Some(2),
        1,
        false,
        None,
    )
    .is_err());
}

#[test]
fn ab_comparison_never_trades_security_or_integrity_for_performance() {
    let baseline = HarnessEvalVariant {
        execution: HarnessExecutionMode::Serial,
        context: HarnessContextStrategy::OldTruncation,
        verification: HarnessVerificationStrategy::Baseline,
    };
    let candidate = HarnessEvalVariant {
        execution: HarnessExecutionMode::Parallel,
        context: HarnessContextStrategy::NewSummary,
        verification: HarnessVerificationStrategy::Extra,
    };
    let accepted = HarnessEvalComparison::new(
        digest('a'),
        baseline,
        candidate,
        digest('b'),
        digest('c'),
        EvalVerdict::Pass,
        EvalVerdict::Pass,
        false,
        false,
        true,
        None,
    )
    .unwrap();
    assert!(accepted.promote);

    let blocked = HarnessEvalComparison::new(
        digest('a'),
        baseline,
        candidate,
        digest('b'),
        digest('c'),
        EvalVerdict::Pass,
        EvalVerdict::Pass,
        true,
        false,
        true,
        Some("permission denial regression".to_owned()),
    )
    .unwrap();
    assert!(!blocked.promote);
    assert_eq!(
        serde_json::to_value(blocked).unwrap()["performance_improved"],
        json!(true)
    );
}
