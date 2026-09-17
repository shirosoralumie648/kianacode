use kiana_commands::eval::{EVAL_REPORT_SCHEMA, EVAL_SUITE_SCHEMA, LEGACY_EVAL_JSON_FIELDS};
use kiana_domain::adapt_legacy_eval_suite;
use serde_json::json;

#[test]
fn legacy_eval_cli_round_trips_through_quality_dto() {
    let legacy = json!({
        "schema": EVAL_SUITE_SCHEMA,
        "id": "legacy-suite",
        "description": "compatibility fixture",
        "cases": [
            {
                "id": "case-1",
                "kind": "runtime_event_replay",
                "fixture": "events/case-1.jsonl",
                "expect": {"final_status":"completed", "min_event_count":1}
            }
        ]
    });
    let first = adapt_legacy_eval_suite(legacy.clone(), "legacy-eval").unwrap();
    let second = adapt_legacy_eval_suite(legacy, "legacy-eval").unwrap();
    assert_eq!(first.suite.suite_id, second.suite.suite_id);
    assert_eq!(first.dataset.dataset_id, second.dataset.dataset_id);
    assert_eq!(first.cases[0].case_id, second.cases[0].case_id);
    assert_eq!(first.suite.schema, EVAL_SUITE_SCHEMA);
    assert!(LEGACY_EVAL_JSON_FIELDS.contains(&"cases"));
    assert_eq!(EVAL_REPORT_SCHEMA, "kiana.eval-report.v1");
}

#[test]
fn legacy_adapter_rejects_unknown_fields_without_changing_report_contract() {
    let mut legacy = json!({
        "schema": EVAL_SUITE_SCHEMA,
        "id": "legacy-suite",
        "cases": [{
            "id": "case-1",
            "kind": "runtime_event_replay",
            "fixture": "events.jsonl",
            "expect": {"min_event_count":1}
        }]
    });
    legacy["removed_field"] = json!(true);
    assert_eq!(
        adapt_legacy_eval_suite(legacy, "legacy-eval").unwrap_err(),
        "legacy_eval_suite_unknown_field"
    );
}
