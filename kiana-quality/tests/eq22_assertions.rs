use kiana_quality::{
    evaluate_assertion, evaluate_assertions, Assertion, AssertionError, AssertionMode,
};
use serde_json::json;

fn assertion(path: &str, mode: AssertionMode, expected: serde_json::Value) -> Assertion {
    Assertion::new(path, mode, expected).expect("valid assertion")
}

#[test]
fn assertion_modes_are_declarative_and_do_not_touch_unrelated_fields() {
    let actual = json!({
        "items": ["a", "b", "a"],
        "ordered": [1, 2, 3],
        "score": 10.05,
        "message": "hello kiana quality",
        "metadata": {"untouched": "stable"}
    });
    let assertions = vec![
        assertion("items", AssertionMode::Multiset, json!(["a", "a", "b"])),
        assertion("ordered", AssertionMode::Ordered, json!([1, 2, 3])),
        assertion(
            "score",
            AssertionMode::NumericTolerance {
                absolute: 0.1,
                relative: 0.0,
            },
            json!(10.0),
        ),
        assertion("message", AssertionMode::Regex, json!("^hello .*quality$")),
        assertion("message", AssertionMode::Contains, json!("kiana")),
        assertion("metadata.untouched", AssertionMode::Exact, json!("stable")),
    ];
    let results = evaluate_assertions(&actual, &assertions).unwrap();
    assert!(results.iter().all(|result| result.passed));
    assert!(results.iter().all(|result| result.code.is_none()));

    let mut changed_unrelated = actual.clone();
    changed_unrelated["metadata"]["new_field"] = json!("ignored");
    assert!(
        evaluate_assertion(&changed_unrelated, &assertions[0])
            .unwrap()
            .passed
    );
}

#[test]
fn ordered_and_multiset_modes_have_distinct_array_semantics() {
    let actual = json!([1, 2, 3]);
    let ordered = assertion("", AssertionMode::Ordered, json!([3, 2, 1]));
    let multiset = assertion("", AssertionMode::Multiset, json!([3, 2, 1]));
    let ordered_result = evaluate_assertion(&actual, &ordered).unwrap();
    let multiset_result = evaluate_assertion(&actual, &multiset).unwrap();
    assert!(!ordered_result.passed);
    assert_eq!(
        ordered_result.code.as_deref(),
        Some("assertion_ordered_mismatch")
    );
    assert!(multiset_result.passed);
}

#[test]
fn failed_assertions_have_stable_codes_and_redacted_summaries() {
    let actual = json!({"text": "Authorization: Bearer actual-secret"});
    let expected = assertion(
        "text",
        AssertionMode::Exact,
        json!("Authorization: Bearer expected-secret"),
    );
    let result = evaluate_assertion(&actual, &expected).unwrap();
    assert!(!result.passed);
    assert_eq!(result.code.as_deref(), Some("assertion_exact_mismatch"));
    assert!(!result.actual_summary.contains("actual-secret"));
    assert!(!result.expected_summary.contains("expected-secret"));

    let missing = assertion("missing", AssertionMode::Exact, json!(true));
    let missing_result = evaluate_assertion(&actual, &missing).unwrap();
    assert_eq!(
        missing_result.code.as_deref(),
        Some("assertion_path_missing")
    );
    assert_eq!(missing_result.actual_summary, "<missing>");
}

#[test]
fn invalid_assertion_specs_fail_closed() {
    assert_eq!(
        Assertion::new("items", AssertionMode::Ordered, json!(true)).unwrap_err(),
        AssertionError::ExpectedArray
    );
    assert_eq!(
        Assertion::new("text", AssertionMode::Regex, json!("[")).unwrap_err(),
        AssertionError::RegexInvalid
    );
    let too_many = vec![assertion("", AssertionMode::Exact, json!(true)); 257];
    assert_eq!(
        evaluate_assertions(&json!(true), &too_many).unwrap_err(),
        AssertionError::AssertionLimitExceeded
    );
}
