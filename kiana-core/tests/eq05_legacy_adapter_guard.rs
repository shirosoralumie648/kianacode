#[test]
fn legacy_eval_adapter_is_explicit_and_report_shape_remains_compatible() {
    let eval = include_str!("../../kiana-commands/src/eval.rs");
    let quality = include_str!("../../kiana-domain/src/quality.rs");
    let manifest = include_str!("../../kiana-commands/Cargo.toml");
    for marker in [
        "suite_value",
        "adapt_legacy_eval_suite",
        "legacy eval quality adapter rejected suite",
        "EVAL_REPORT_SCHEMA",
        "EvalReport",
    ] {
        assert!(
            eval.contains(marker),
            "eval adapter marker missing: {marker}"
        );
    }
    for marker in [
        "pub fn adapt_legacy_eval_suite",
        "LegacyEvalQualityBundle",
        "stable_quality_uuid",
        "legacy_eval_suite_unknown_field",
    ] {
        assert!(
            quality.contains(marker),
            "quality adapter marker missing: {marker}"
        );
    }
    assert!(manifest.contains("kiana-domain"));
    assert!(!eval.contains("QualityGate"));
    assert!(!eval.contains("authorize_and_execute"));
}
