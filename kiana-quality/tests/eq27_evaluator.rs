use kiana_quality::{
    canonical_findings, digest_findings, sort_findings, validate_finding, DeterministicEvaluator,
    EvaluationInput, EvaluationResult, EvaluatorError, EvaluatorRegistry, Finding, FindingError,
    EVALUATION_RESULT_SCHEMA, EVALUATOR_INPUT_SCHEMA, FINDING_SCHEMA, MAX_EVALUATORS,
    MAX_EVALUATOR_INPUT_BYTES, MAX_FINDINGS, MAX_FINDING_CODE_BYTES,
    MAX_FINDING_EVIDENCE_REF_BYTES, MAX_FINDING_MESSAGE_BYTES, MAX_FINDING_VALUE_BYTES,
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

struct IdEvaluator {
    id: String,
}

impl IdEvaluator {
    fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

impl DeterministicEvaluator for IdEvaluator {
    fn evaluator_id(&self) -> &str {
        &self.id
    }

    fn evaluate(&self, _input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        Ok(Vec::new())
    }
}

/// Emits a finding that bypassed [`Finding::new`], so the registry has to reject it.
struct TamperedEvaluator {
    code: &'static str,
    evidence_ref: &'static str,
}

impl DeterministicEvaluator for TamperedEvaluator {
    fn evaluator_id(&self) -> &str {
        "tampered"
    }

    fn evaluate(&self, _input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        Ok(vec![Finding {
            schema: FINDING_SCHEMA.to_owned(),
            code: self.code.to_owned(),
            expected: json!({"terminal": 1}),
            actual: json!({"terminal": 2}),
            message: "finding that bypassed the constructor".to_owned(),
            evidence_ref: self.evidence_ref.to_owned(),
        }])
    }
}

struct BulkEvaluator;

impl DeterministicEvaluator for BulkEvaluator {
    fn evaluator_id(&self) -> &str {
        "bulk"
    }

    fn evaluate(&self, _input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        Ok((0..=MAX_FINDINGS)
            .map(|index| {
                Finding::new(
                    format!("bulk.{index:05}"),
                    json!(index),
                    json!(0),
                    "bulk finding",
                    format!("event:{index}"),
                )
                .expect("bounded fixture finding")
            })
            .collect())
    }
}

#[test]
fn every_finding_requires_a_stable_code_and_evidence_ref() {
    for code in [
        "",
        "Runtime.Case",
        "runtime case",
        "runtime/case",
        ".runtime",
        "runtime.",
        "运行时",
    ] {
        assert_eq!(
            Finding::new(
                code,
                json!(true),
                json!(false),
                "bounded message",
                "event:1"
            )
            .unwrap_err(),
            FindingError::CodeInvalid,
            "unstable finding code accepted: {code:?}"
        );
    }
    assert_eq!(
        Finding::new(
            "c".repeat(MAX_FINDING_CODE_BYTES + 1),
            json!(true),
            json!(false),
            "bounded message",
            "event:1",
        )
        .unwrap_err(),
        FindingError::CodeInvalid
    );

    for evidence_ref in ["", "event:1 2", "event:1\n2", "event:1\t2"] {
        assert_eq!(
            Finding::new(
                "runtime.terminal",
                json!(true),
                json!(false),
                "bounded message",
                evidence_ref,
            )
            .unwrap_err(),
            FindingError::EvidenceRefInvalid,
            "unstable evidence reference accepted: {evidence_ref:?}"
        );
    }
    assert_eq!(
        Finding::new(
            "runtime.terminal",
            json!(true),
            json!(false),
            "bounded message",
            "e".repeat(MAX_FINDING_EVIDENCE_REF_BYTES + 1),
        )
        .unwrap_err(),
        FindingError::EvidenceRefInvalid
    );

    for message in ["  ", "line\nbreak", "line\rbreak"] {
        assert_eq!(
            Finding::new(
                "runtime.terminal",
                json!(true),
                json!(false),
                message,
                "event:1"
            )
            .unwrap_err(),
            FindingError::MessageInvalid,
            "unbounded finding message accepted: {message:?}"
        );
    }
    assert_eq!(
        Finding::new(
            "runtime.terminal",
            json!(true),
            json!(false),
            "m".repeat(MAX_FINDING_MESSAGE_BYTES + 1),
            "event:1",
        )
        .unwrap_err(),
        FindingError::MessageInvalid
    );
    assert_eq!(
        Finding::new(
            "runtime.terminal",
            json!(true),
            json!(false),
            "Authorization: Bearer secret-value",
            "event:1",
        )
        .unwrap_err(),
        FindingError::SecretDetected("message")
    );
    assert_eq!(
        Finding::new(
            "runtime.terminal",
            json!("x".repeat(MAX_FINDING_VALUE_BYTES + 1)),
            json!(false),
            "bounded message",
            "event:1",
        )
        .unwrap_err(),
        FindingError::ValueTooLarge("expected")
    );

    let finding = Finding::new(
        "runtime.terminal",
        json!({"count": 1}),
        json!({"count": 2}),
        "terminal count mismatch",
        "event:1#terminal",
    )
    .expect("valid finding");
    assert_eq!(finding.schema, FINDING_SCHEMA);
    assert_eq!(finding.code, "runtime.terminal");
    assert_eq!(finding.evidence_ref, "event:1#terminal");
    assert!(validate_finding(&finding).is_ok());

    let mut stale_schema = finding.clone();
    stale_schema.schema = "kiana.quality-finding.v2".to_owned();
    assert_eq!(
        stale_schema.validate().unwrap_err(),
        FindingError::SchemaInvalid
    );
}

#[test]
fn registry_never_returns_a_finding_without_a_stable_code_or_evidence_ref() {
    for (code, evidence_ref, expected) in [
        ("Runtime.Case", "event:1", FindingError::CodeInvalid),
        ("runtime.terminal", "", FindingError::EvidenceRefInvalid),
        (
            "runtime.terminal",
            "event:1 2",
            FindingError::EvidenceRefInvalid,
        ),
    ] {
        let mut registry = EvaluatorRegistry::new();
        registry
            .register(TamperedEvaluator { code, evidence_ref })
            .expect("register the tampered evaluator");
        assert_eq!(
            registry.evaluate(&json!({"case": "fixture"})).unwrap_err(),
            EvaluatorError::Finding(expected)
        );
        assert_eq!(registry.evaluator_ids(), vec!["tampered".to_owned()]);
    }
}

#[test]
fn registry_limits_and_input_bounds_fail_closed() {
    let mut registry = EvaluatorRegistry::new();
    for index in 0..MAX_EVALUATORS {
        registry
            .register(IdEvaluator::new(format!("e{index}")))
            .expect("register a bounded evaluator");
    }
    assert_eq!(registry.len(), MAX_EVALUATORS);
    assert_eq!(
        registry.register(IdEvaluator::new("overflow")).unwrap_err(),
        EvaluatorError::EvaluatorLimitExceeded
    );

    let mut registry = EvaluatorRegistry::new();
    assert_eq!(
        registry
            .register(IdEvaluator::new("Runtime Case"))
            .unwrap_err(),
        EvaluatorError::EvaluatorIdInvalid
    );
    assert!(registry.is_empty());
    registry
        .register(IdEvaluator::new("bounded"))
        .expect("valid evaluator id");
    assert_eq!(
        registry
            .evaluate(&json!({"blob": "x".repeat(MAX_EVALUATOR_INPUT_BYTES)}))
            .unwrap_err(),
        EvaluatorError::InputTooLarge
    );

    let mut registry = EvaluatorRegistry::new();
    registry
        .register(BulkEvaluator)
        .expect("register the bulk evaluator");
    assert_eq!(
        registry.evaluate(&json!({"case": "fixture"})).unwrap_err(),
        EvaluatorError::FindingLimitExceeded
    );
}

#[test]
fn finding_digest_binds_every_evidence_ref_and_value() {
    let base = finding(
        "runtime.terminal",
        json!({"count": 1}),
        json!({"count": 2}),
        "event:1",
    );
    let base_digest = digest_findings(&[base.clone()]).expect("finding digest");
    assert_eq!(
        base_digest,
        digest_findings(&[base.clone()]).expect("stable digest")
    );

    for changed in [
        finding(
            "runtime.terminal",
            json!({"count": 1}),
            json!({"count": 2}),
            "event:2",
        ),
        finding(
            "runtime.terminal",
            json!({"count": 9}),
            json!({"count": 2}),
            "event:1",
        ),
        finding(
            "runtime.terminal",
            json!({"count": 1}),
            json!({"count": 9}),
            "event:1",
        ),
        finding(
            "runtime.other",
            json!({"count": 1}),
            json!({"count": 2}),
            "event:1",
        ),
    ] {
        assert_ne!(
            base_digest,
            digest_findings(&[changed]).expect("finding digest"),
            "the digest must bind every finding field, including its evidence reference"
        );
    }

    let unordered = vec![
        finding("runtime.z", json!(2), json!(3), "event:3"),
        finding("runtime.a", json!(1), json!(2), "event:1"),
    ];
    let canonical = canonical_findings(unordered.clone()).expect("canonical findings");
    assert_eq!(canonical[0].code, "runtime.a");
    assert_eq!(canonical[1].code, "runtime.z");
    assert_eq!(
        digest_findings(&unordered).expect("finding digest"),
        digest_findings(&canonical).expect("finding digest")
    );

    let result =
        EvaluationResult::new(vec!["runtime".to_owned()], canonical.clone()).expect("result");
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert!(result.validate().is_ok());

    let mut tampered = result.clone();
    tampered.findings_digest = "0".repeat(64);
    assert_eq!(
        tampered.validate().unwrap_err(),
        FindingError::ValueInvalid("findings_digest")
    );
    assert_eq!(
        EvaluationResult::new(
            vec!["runtime".to_owned(), "runtime".to_owned()],
            canonical.clone(),
        )
        .unwrap_err(),
        FindingError::ValueInvalid("evaluator_ids")
    );
    assert_eq!(
        EvaluationResult::new(Vec::new(), canonical).unwrap_err(),
        FindingError::ValueInvalid("evaluator_ids")
    );
}

#[test]
fn evaluator_input_is_versioned_and_bounded() {
    let input = EvaluationInput::new(json!({"terminal": 1}), json!({"terminal": 2}), "event:1")
        .expect("valid input");
    assert_eq!(input.schema, EVALUATOR_INPUT_SCHEMA);
    assert_eq!(input.as_value()["schema"], EVALUATOR_INPUT_SCHEMA);

    assert_eq!(
        EvaluationInput::new(json!(true), json!(false), "event:1 2").unwrap_err(),
        EvaluatorError::Finding(FindingError::EvidenceRefInvalid)
    );
    assert_eq!(
        EvaluationInput::new(
            json!("x".repeat(MAX_EVALUATOR_INPUT_BYTES)),
            json!(false),
            "event:1",
        )
        .unwrap_err(),
        EvaluatorError::InputTooLarge
    );

    let valid = EvaluationInput::new(json!(true), json!(false), "event:1").expect("valid input");

    let mut unknown = serde_json::to_value(&valid).expect("serialize input");
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<EvaluationInput>(unknown).is_err());

    let mut stale = serde_json::to_value(&valid).expect("serialize input");
    stale["schema"] = json!("kiana.quality-evaluator-input.v2");
    let stale: EvaluationInput = serde_json::from_value(stale).expect("decode input");
    assert_eq!(
        stale.validate().unwrap_err(),
        EvaluatorError::InputSchemaInvalid
    );
}
