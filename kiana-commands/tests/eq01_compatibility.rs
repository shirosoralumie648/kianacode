use kiana_commands::eval::{
    EVAL_BASELINE_SCHEMA, EVAL_MAX_CASES, EVAL_MAX_FIXTURE_BYTES, EVAL_MAX_FIXTURE_LINES,
    EVAL_REPORT_SCHEMA, EVAL_SUITE_SCHEMA, LEGACY_EVAL_ERROR_CODES,
    LEGACY_EVAL_JSON_COMPATIBILITY_TESTS, LEGACY_EVAL_JSON_FIELDS,
};

#[test]
fn legacy_eval_v1_contract_is_pinned() {
    assert_eq!(EVAL_SUITE_SCHEMA, "kiana.eval-suite.v1");
    assert_eq!(EVAL_REPORT_SCHEMA, "kiana.eval-report.v1");
    assert_eq!(EVAL_BASELINE_SCHEMA, "kiana.eval-baseline.v1");
    assert_eq!(EVAL_MAX_CASES, 256);
    assert_eq!(EVAL_MAX_FIXTURE_BYTES, 16 * 1024 * 1024);
    assert_eq!(EVAL_MAX_FIXTURE_LINES, 100_000);
    for field in [
        "schema",
        "suite_id",
        "suite_sha256",
        "status",
        "summary",
        "baseline",
        "cases",
        "metrics",
        "event_count",
        "tool_call_count",
        "tool_error_count",
        "input_tokens",
        "output_tokens",
        "final_status",
        "stop_reason",
        "findings",
        "code",
        "expected",
        "actual",
        "message",
    ] {
        assert!(
            LEGACY_EVAL_JSON_FIELDS.contains(&field),
            "legacy field removed: {field}"
        );
    }
    for code in [
        "baseline_case_missing_from_suite",
        "baseline_threshold_exceeded",
        "baseline_value_mismatch",
        "eval_fixture_empty",
        "eval_fixture_invalid_json",
    ] {
        assert!(
            LEGACY_EVAL_ERROR_CODES.contains(&code),
            "legacy error removed: {code}"
        );
    }
    for fixture in [
        "eval_run_reports_deterministic_runtime_metrics",
        "eval_run_compares_local_baseline_thresholds",
        "eval_run_fails_when_baseline_regresses",
        "eval_rejects_invalid_baseline_contracts",
        "eval_rejects_invalid_suite_and_fixture_contracts",
        "eval_rejects_symlink_fixture_escape",
    ] {
        assert!(
            LEGACY_EVAL_JSON_COMPATIBILITY_TESTS.contains(&fixture),
            "compatibility fixture not registered: {fixture}"
        );
    }
}
