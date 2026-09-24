use kiana_quality::{
    digest_findings, sort_findings, DeterministicEvaluator, EvaluatorError, EvaluatorRegistry,
    Finding, FindingError, FINDING_SCHEMA,
};
use serde_json::{json, Value};

fn finding(code: &str, expected: Value, actual: Value, evidence_ref: &str) -> Finding {
    Finding::new(
        code,
        expected,
        actual,
        format!("finding {code}"),
        evidence_ref,
    )
    .expect("valid finding")
}

struct UnorderedEvaluator {
    id: &'static str,
    reverse: bool,
}

impl DeterministicEvaluator for UnorderedEvaluator {
    fn evaluator_id(&self) -> &str {
        self.id
    }

    fn evaluate(&self, _input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let mut findings = vec![
            finding("runtime.z", json!(2), json!(3), "event:2"),
            finding("runtime.a", json!(1), json!(2), "event:1"),
        ];
        if self.reverse {
            findings.reverse();
        }
        Ok(findings)
    }
}

#[test]
fn every_finding_has_stable_code_and_evidence() {
    let mut registry = EvaluatorRegistry::new();
    registry
        .register(UnorderedEvaluator {
            id: "runtime",
            reverse: true,
        })
        .unwrap();

    let findings = registry.evaluate(&json!({"case": "fixture"})).unwrap();
    assert_eq!(findings[0].schema, FINDING_SCHEMA);
    assert_eq!(findings[0].code, "runtime.a");
    assert_eq!(findings[0].evidence_ref, "event:1");
    assert!(findings
        .iter()
        .all(|finding| finding.validate().is_ok() && !finding.code.is_empty()));
    assert_eq!(
        digest_findings(&findings).unwrap(),
        digest_findings(&findings).unwrap()
    );
}

#[test]
fn registry_and_sorting_are_independent_of_registration_or_emission_order() {
    let mut left = EvaluatorRegistry::new();
    left.register(UnorderedEvaluator {
        id: "second",
        reverse: false,
    })
    .unwrap();
    left.register(UnorderedEvaluator {
        id: "first",
        reverse: true,
    })
    .unwrap();

    let mut right = EvaluatorRegistry::new();
    right
        .register(UnorderedEvaluator {
            id: "first",
            reverse: false,
        })
        .unwrap();
    right
        .register(UnorderedEvaluator {
            id: "second",
            reverse: true,
        })
        .unwrap();

    let left_findings = left.evaluate(&json!({"case": "fixture"})).unwrap();
    let right_findings = right.evaluate(&json!({"case": "fixture"})).unwrap();
    assert_eq!(left_findings, right_findings);

    let mut shuffled = right_findings.clone();
    shuffled.reverse();
    sort_findings(&mut shuffled).unwrap();
    assert_eq!(shuffled, right_findings);
}

#[test]
fn finding_rejects_unknown_schema_secrets_and_unbounded_values() {
    let mut unknown: serde_json::Value =
        serde_json::to_value(finding("safe.code", json!(true), json!(false), "fixture:1")).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<Finding>(unknown).is_err());

    let secret = Finding::new(
        "safe.code",
        json!({"api_key": "secret-value"}),
        json!(false),
        "secret must not cross the finding boundary",
        "fixture:1",
    );
    assert_eq!(
        secret.unwrap_err(),
        FindingError::SecretDetected("expected")
    );

    let redacted = Finding::redacted(
        "safe.code",
        json!({"api_key": "secret-value"}),
        json!(false),
        "Authorization: Bearer secret-value",
        "fixture:1",
    )
    .unwrap();
    assert_eq!(redacted.expected["api_key"], "[REDACTED]");
    assert!(!serde_json::to_string(&redacted)
        .unwrap()
        .contains("secret-value"));

    let too_deep = (0..=kiana_quality::MAX_FINDING_VALUE_DEPTH + 1)
        .fold(json!(true), |value, _| json!([value]));
    assert_eq!(
        Finding::new("safe.code", too_deep, json!(false), "bounded", "fixture:1").unwrap_err(),
        FindingError::ValueTooDeep("expected")
    );
}

#[test]
fn duplicate_evaluator_ids_fail_closed() {
    let mut registry = EvaluatorRegistry::new();
    registry
        .register(UnorderedEvaluator {
            id: "duplicate",
            reverse: false,
        })
        .unwrap();
    assert_eq!(
        registry
            .register(UnorderedEvaluator {
                id: "duplicate",
                reverse: true,
            })
            .unwrap_err(),
        EvaluatorError::DuplicateEvaluator
    );
}
