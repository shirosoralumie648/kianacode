use kiana_domain::*;
use serde_json::json;
use std::collections::BTreeMap;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

#[test]
fn eval_dataset_suite_case_and_golden_trace_bind_schema_and_provenance() {
    let case_id = EvalCaseId::new();
    let dataset = EvalDataset::new(
        "regression",
        "quality-owner",
        vec!["fixture:source".to_owned()],
        "internal",
        EvalSplit::Regression,
        vec![case_id],
        100,
        Some(200),
    )
    .unwrap();
    dataset.validate().unwrap();

    let suite = EvalSuite::new(
        dataset.dataset_id,
        "offline",
        "replay",
        vec![case_id],
        vec!["evaluator:runtime".to_owned()],
        "policy:score",
        "policy:safety",
        "policy:budget",
        None,
        "kiana.eval-case.v1",
        "quality-owner",
    )
    .unwrap();
    suite.validate().unwrap();
    assert_eq!(
        suite.status.transition(EvalSuiteStatus::Active).unwrap(),
        EvalSuiteStatus::Active
    );
    assert_eq!(
        EvalSuiteStatus::Active
            .transition(EvalSuiteStatus::Draft)
            .unwrap_err(),
        "eval_suite_transition_invalid"
    );

    let case = EvalCase::new(
        suite.suite_id,
        "fixture:case-1",
        Some("fixture:initial-1".to_owned()),
        json!({"prompt":"hello","mode":"replay"}),
        vec!["run.completed".to_owned()],
        "completed",
        vec!["artifact:output".to_owned()],
        vec!["receipt.status=completed".to_owned()],
        vec!["network".to_owned()],
        vec!["status_is_terminal".to_owned()],
        "internal",
    )
    .unwrap();
    case.validate().unwrap();
    assert_eq!(case.case_id, case_id);

    let mut target_versions = BTreeMap::new();
    target_versions.insert("runtime".to_owned(), "v1".to_owned());
    let trace = GoldenTrace::new(
        suite.suite_id,
        case.case_id,
        Some(RunId::new()),
        "source:commit",
        digest('a'),
        target_versions,
        1,
        2,
        vec![json!({"kind":"run.completed","sequence":1})],
        vec![digest('b')],
        Some(digest('c')),
        "normalization.v1",
        Some(true),
        Some(0.95),
        100,
        Some(200),
        "provenance:capture-1",
    )
    .unwrap();
    trace.validate().unwrap();
    let bytes = canonical_quality_bytes(&trace).unwrap();
    let decoded: GoldenTrace = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, trace);
    assert_eq!(
        schema_contract(GOLDEN_TRACE_SCHEMA).unwrap().owner_crate,
        "kiana-domain"
    );
}

#[test]
fn quality_objects_reject_unknown_schema_fields_missing_provenance_and_secret_config() {
    let case_id = EvalCaseId::new();
    assert_eq!(
        EvalDataset::new(
            "regression",
            "owner",
            Vec::new(),
            "internal",
            EvalSplit::Regression,
            vec![case_id],
            1,
            None,
        )
        .unwrap_err(),
        "eval_dataset_provenance_invalid"
    );
    let dataset = EvalDataset::new(
        "regression",
        "owner",
        vec!["fixture:source".to_owned()],
        "internal",
        EvalSplit::Regression,
        vec![case_id],
        1,
        None,
    )
    .unwrap();
    let suite = EvalSuite::new(
        dataset.dataset_id,
        "offline",
        "replay",
        vec![case_id],
        vec!["evaluator:runtime".to_owned()],
        "score",
        "safety",
        "budget",
        None,
        "kiana.eval-case.v1",
        "owner",
    )
    .unwrap();
    assert_eq!(
        EvalCase::new(
            suite.suite_id,
            "fixture:case",
            None,
            json!({"token":"secret-value"}),
            Vec::new(),
            "blocked",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            "internal",
        )
        .unwrap_err(),
        "eval_case_target_config_invalid"
    );
    let mut encoded = serde_json::to_value(&dataset).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<EvalDataset>(encoded).is_err());
    let mut bad_schema = dataset.clone();
    bad_schema.schema = "kiana.eval-dataset.v9".to_owned();
    assert_eq!(
        bad_schema.validate().unwrap_err(),
        "eval_dataset_schema_invalid"
    );
}

#[test]
fn golden_trace_rejects_cursor_and_expiry_regressions() {
    let suite_id = EvalSuiteId::new();
    let case_id = EvalCaseId::new();
    let mut target_versions = BTreeMap::new();
    target_versions.insert("runtime".to_owned(), "v1".to_owned());
    assert_eq!(
        GoldenTrace::new(
            suite_id,
            case_id,
            None,
            "source",
            digest('a'),
            target_versions.clone(),
            2,
            1,
            vec![json!({"kind":"run.failed"})],
            Vec::new(),
            None,
            "normalization.v1",
            None,
            None,
            100,
            Some(99),
            "provenance",
        )
        .unwrap_err(),
        "golden_trace_header_invalid"
    );
}
