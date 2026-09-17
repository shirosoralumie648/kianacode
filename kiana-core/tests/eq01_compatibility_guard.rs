#[test]
fn legacy_eval_parser_uses_extracted_contract_constants() {
    let source = include_str!("../../kiana-commands/src/eval.rs");
    for marker in [
        "pub const EVAL_SUITE_SCHEMA",
        "pub const EVAL_REPORT_SCHEMA",
        "pub const EVAL_BASELINE_SCHEMA",
        "pub const LEGACY_EVAL_JSON_FIELDS",
        "pub const LEGACY_EVAL_ERROR_CODES",
        "pub const LEGACY_EVAL_JSON_COMPATIBILITY_TESTS",
        "EVAL_SUITE_SCHEMA",
        "EVAL_REPORT_SCHEMA",
        "EVAL_BASELINE_SCHEMA",
        "EVAL_MAX_CASES",
        "EVAL_MAX_FIXTURE_BYTES",
        "EVAL_MAX_FIXTURE_LINES",
        "serde(deny_unknown_fields)",
    ] {
        assert!(
            source.contains(marker),
            "legacy eval contract marker missing: {marker}"
        );
    }
    assert!(!source.contains("const SUITE_SCHEMA:"));
    assert!(!source.contains("const REPORT_SCHEMA:"));
    assert!(!source.contains("const BASELINE_SCHEMA:"));
}
