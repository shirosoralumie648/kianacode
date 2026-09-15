#[test]
fn eval_baseline_distinguishes_legacy_command_runner_from_core_projection() {
    let legacy = include_str!("../../kiana-commands/src/eval.rs");
    let core = include_str!("../src/eval.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let release = include_str!("../../scripts/release-smoke.sh");

    assert!(legacy.contains("kiana.eval-suite.v1"));
    assert!(legacy.contains("kiana.eval-report.v1"));
    assert!(legacy.contains("run_suite"));
    assert!(legacy.contains("fixture_sha256"));
    assert!(cli.contains("create_default_command_registry"));
    assert!(cli.contains("execute_command"));
    assert!(release.contains("smoke_eval_json"));

    assert!(core.contains("evaluate_provider_independent"));
    assert!(core.contains("rebuild_audit_projection"));
    assert!(core.contains("project_operational_metrics"));
    assert!(core.contains("diagnose_replay"));
    assert!(core.contains("read_all_events"));
    assert!(!core.contains("ModelClient"));
    assert!(!core.contains("CapabilityBrokerPort"));
    assert!(!core.contains("authorize_and_execute"));
}

#[test]
fn eval_baseline_pins_missing_quality_platform_and_migration_work() {
    let baseline = include_str!("../../docs/roadmap/evaluation-baseline.md");
    for required in [
        "partial",
        "local_behavior",
        "legacy EvalCommand",
        "provider-independent",
        "EvalStore",
        "TraceNormalizer",
        "QualityGate",
        "scripts/eval-curated.sh",
        "second execution loop",
        "EQ-01",
    ] {
        assert!(baseline.contains(required), "baseline missing {required}");
    }
}
