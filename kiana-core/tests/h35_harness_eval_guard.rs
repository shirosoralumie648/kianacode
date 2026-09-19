//! H35 source guard for Harness GoldenTrace/eval/performance evidence.

#[test]
fn harness_eval_is_replay_bound_and_side_effect_free() {
    let quality = include_str!("../../kiana-domain/src/quality.rs");
    let eval = include_str!("../../kiana-domain/src/eval.rs");
    let harness_eval = include_str!("../../kiana-domain/src/harness_eval.rs");
    let performance = include_str!("../../kiana-domain/src/performance.rs");
    let versioning = include_str!("../src/versioning.rs");
    let evidence = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    let baseline = include_str!("../../docs/roadmap/h35-harness-eval-baseline.md");

    for marker in [
        "GoldenTrace",
        "GOLDEN_TRACE_SCHEMA",
        "normalized_events",
        "source_snapshot",
        "input_hash",
        "events_hash",
        "receipt_hash",
        "target_versions",
        "EvalCaseSpec",
        "EvalCaseResult",
        "EvalSuite",
        "replay_diverged",
        "source_cursor",
        "cost_kind",
        "latency_bucket",
        "BenchmarkSummary",
        "PerformanceBaseline",
        "CapacityEnvelope",
        "MigrationObservation",
        "trace.replay",
        "trace_digest_mismatch",
        "trace_data_revoked",
        "evaluate_provider_independent",
        "side_effects",
        "provider_calls",
        "EvalEvidenceCapture",
        "infra_flush_unknown",
        "HarnessTraceBinding",
        "tool_directory_hash",
        "workspace_hash",
        "HarnessReplayReport",
        "OfflineNoEffects",
        "MissingResult",
        "ExtraSideEffect",
        "HarnessEvalMetric",
        "CancellationLatency",
        "UsageCompleteness",
        "HarnessEvalComparison",
        "security_regression",
        "result_integrity_regression",
        "performance_improved",
        "harness_eval_comparison_promote_mismatch",
    ] {
        assert!(
            quality.contains(marker)
                || eval.contains(marker)
                || harness_eval.contains(marker)
                || performance.contains(marker)
                || versioning.contains(marker)
                || evidence.contains(marker),
            "H35 marker missing: {marker}"
        );
    }
    for marker in [
        "golden_trace_detects_missing_result_or_extra_side_effect",
        "eval_replay_has_no_network_or_process_execution",
        "task_and_context_regression_suite_passes_on_pinned_snapshot",
        "partial",
        "real Provider",
        "performance",
        "no local runtime",
    ] {
        assert!(
            baseline.contains(marker),
            "H35 baseline marker missing: {marker}"
        );
    }
    assert!(!versioning.contains("ModelClient"));
    assert!(!versioning.contains("CapabilityBroker"));
    assert!(!versioning.contains("authorize_and_execute"));
}
