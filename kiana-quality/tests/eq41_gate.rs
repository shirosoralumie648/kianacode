use kiana_quality::{
    canonical_findings, evaluate_gate, evaluate_gate_log, quality_gate_config_update,
    quality_gate_decision_binding, DeterministicEvaluator, EvaluatorRegistry, Finding,
    GateConfigHistory, GateEvaluator, GateInput, GateLogEvaluator, GateLogInput, GateVerdict,
    QualityGateConfig, QualityGateDecision, QualityGateDecisionLog, GATE_BLOCKING_RULES,
    GATE_CONFIG_SCHEMA, GATE_DECISION_SCHEMA, GATE_EVALUATOR_ID, GATE_INPUT_SCHEMA,
    GATE_LOG_EVALUATOR_ID, GATE_LOG_INPUT_SCHEMA, GATE_THRESHOLD_DIMENSIONS,
    MAX_GATE_BLOCKING_RULES, MAX_GATE_EVIDENCE_REFS, MAX_GATE_SCORE_MILLI, MAX_GATE_TEXT_BYTES,
    MAX_GATE_THRESHOLDS,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn sha(hex: char) -> String {
    format!("sha256:{}", hex.to_string().repeat(64))
}

fn thresholds() -> BTreeMap<String, u64> {
    BTreeMap::from([
        ("evidence_completeness".to_owned(), 900),
        ("policy_safety".to_owned(), 1_000),
        ("replay_correctness".to_owned(), 950),
    ])
}

fn rules() -> Vec<String> {
    vec!["infra".to_owned(), "safety".to_owned()]
}

fn config(version: u64, suite_digest: &str) -> QualityGateConfig {
    QualityGateConfig::new("gate:quality", version, suite_digest, thresholds(), rules())
        .expect("valid gate config version")
}

fn pass_decision(config: &QualityGateConfig) -> QualityGateDecision {
    QualityGateDecision::new(
        "decision:one",
        config,
        "candidate:one",
        sha('9'),
        GateVerdict::Pass,
        120,
        Vec::new(),
        Vec::new(),
        "evidence:run:one",
        41,
        1_000,
    )
    .expect("valid pass decision")
}

fn reject_decision(
    config: &QualityGateConfig,
    decision_id: &str,
    candidate_id: &str,
    source_cursor: u64,
) -> QualityGateDecision {
    QualityGateDecision::new(
        decision_id,
        config,
        candidate_id,
        sha('8'),
        GateVerdict::Reject,
        0,
        vec!["safety".to_owned()],
        Vec::new(),
        "evidence:run:reject",
        source_cursor,
        2_000,
    )
    .expect("valid reject decision")
}

fn reseal_decision(decision: &mut QualityGateDecision) {
    decision.decision_digest = decision.digest();
}

fn reseal_log(log: &mut QualityGateDecisionLog) {
    log.log_digest = log.digest();
}

fn input(config: QualityGateConfig, decision: QualityGateDecision) -> Value {
    GateInput::new(config, decision).as_value()
}

fn log_input(history: GateConfigHistory, log: QualityGateDecisionLog) -> Value {
    GateLogInput::new(history, log).as_value()
}

fn decoded_input(value: &Value) -> GateInput {
    serde_json::from_value(value.clone()).expect("gate input envelope")
}

fn decoded_log_input(value: &Value) -> GateLogInput {
    serde_json::from_value(value.clone()).expect("gate log input envelope")
}

fn evaluate(value: &Value) -> Vec<Finding> {
    GateEvaluator.evaluate(value).expect("gate evaluation")
}

fn evaluate_log(value: &Value) -> Vec<Finding> {
    GateLogEvaluator
        .evaluate(value)
        .expect("gate log evaluation")
}

fn codes(findings: &[Finding]) -> Vec<&str> {
    findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

fn config_reason(config: &QualityGateConfig) -> Option<&'static str> {
    config.validate().err()
}

fn decision_reason(decision: &QualityGateDecision) -> Option<&'static str> {
    decision.validate().err()
}

fn history_reason(history: &GateConfigHistory) -> Option<&'static str> {
    history.validate().err()
}

fn log_reason(log: &QualityGateDecisionLog) -> Option<&'static str> {
    log.validate().err()
}

#[test]
fn gate_config_update_does_not_mutate_old_decision() {
    let v1 = config(1, &sha('1'));
    let decision = pass_decision(&v1);
    let decision_digest = decision.decision_digest.clone();
    let v2 = config(2, &sha('2'));

    // A legal update leaves the old decision byte-identical and still bound to v1.
    let value = GateInput::new(v1.clone(), decision.clone())
        .with_update(v2.clone())
        .as_value();
    assert!(evaluate(&value).is_empty(), "unexpected findings: {value}");
    assert_eq!(decision.decision_digest, decision_digest);
    assert_eq!(decision.config_digest, v1.config_digest);
    assert_eq!(decision.gate_version, 1);
    assert_ne!(v1.config_digest, v2.config_digest);
    assert_eq!(quality_gate_decision_binding(&v1, &decision), Ok(()));

    // An update that reuses the published version is a rewrite, not an update.
    let rewritten = config(1, &sha('2'));
    let value = GateInput::new(v1.clone(), decision.clone())
        .with_update(rewritten)
        .as_value();
    let findings = evaluate(&value);
    assert_eq!(codes(&findings), vec!["gate.config_update_invalid"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_config_update_version_not_incremented")
    );

    // Rebinding the old decision to the new config digest is refused.
    let mut rebound = decision.clone();
    rebound.config_digest = v2.config_digest.clone();
    reseal_decision(&mut rebound);
    let findings = evaluate(&input(v1.clone(), rebound));
    assert_eq!(codes(&findings), vec!["gate.decision_config_mismatch"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_config_digest_mismatch")
    );

    // The append-only log refuses to rewrite the decision ...
    let mut history = GateConfigHistory::new("gate:quality");
    assert_eq!(history.append(v1.clone()), Ok(true));
    assert_eq!(history.append(v2.clone()), Ok(true));
    let mut log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(log.append(decision.clone()), Ok(true));
    let sealed_log = log.clone();
    let mut flipped = decision.clone();
    flipped.verdict = GateVerdict::Reject;
    flipped.blocking_findings = vec!["safety".to_owned()];
    reseal_decision(&mut flipped);
    assert_eq!(log.append(flipped), Err("gate_decision_rewritten"));

    // ... and to re-decide the same candidate under the same config version.
    let redecided = QualityGateDecision::new(
        "decision:two",
        &v1,
        "candidate:one",
        sha('9'),
        GateVerdict::Reject,
        0,
        vec!["safety".to_owned()],
        Vec::new(),
        "evidence:run:two",
        42,
        2_000,
    )
    .expect("redecision under v1");
    assert_eq!(
        log.append(redecided),
        Err("gate_decision_candidate_redecided")
    );
    assert_eq!(log, sealed_log);
    assert_eq!(log.decisions.len(), 1);

    // A newer config version may append a new decision about the same candidate.
    let under_v2 = QualityGateDecision::new(
        "decision:three",
        &v2,
        "candidate:one",
        sha('9'),
        GateVerdict::Reject,
        0,
        vec!["safety".to_owned()],
        Vec::new(),
        "evidence:run:three",
        43,
        3_000,
    )
    .expect("redecision under v2");
    assert_eq!(log.append(under_v2), Ok(true));
    assert_eq!(log.decisions.len(), 2);
    assert!(evaluate_log(&log_input(history, log)).is_empty());
}

#[test]
fn gate_config_and_decision_are_separate_value_contracts() {
    let v1 = config(1, &sha('1'));
    // A new config version must bind a different suite, otherwise the identity separation this
    // test asserts would be vacuous.
    let v2 = config(2, &sha('2'));
    let decision = pass_decision(&v1);

    let config_value = serde_json::to_value(&v1).expect("config value");
    let decision_value = serde_json::to_value(&decision).expect("decision value");
    assert_eq!(config_value["schema"], json!(GATE_CONFIG_SCHEMA));
    assert_eq!(decision_value["schema"], json!(GATE_DECISION_SCHEMA));
    assert!(config_value.get("verdict").is_none());
    assert!(config_value.get("candidate_id").is_none());
    assert!(config_value.get("decided_at_unix_ms").is_none());
    assert!(decision_value.get("thresholds").is_none());
    assert!(decision_value.get("blocking_rules").is_none());
    assert!(decision_value.get("config_digest").is_some());

    // The decision cites the configuration version; it never embeds a copy of it.
    assert_eq!(decision.gate_id, v1.gate_id);
    assert_eq!(decision.gate_version, v1.version);
    assert_eq!(decision.config_digest, v1.config_digest);
    assert_eq!(decision.suite_digest, v1.suite_digest);

    // Two versions of one gate keep the identity and separate their identities.
    assert_eq!(v2.gate_id, v1.gate_id);
    assert_ne!(v2.suite_digest, v1.suite_digest);
    assert_ne!(v2.config_digest, v1.config_digest);
    assert_ne!(v2.thresholds_digest(), v1.thresholds_digest());
    assert_ne!(v2.digest(), v1.digest());
}

#[test]
fn gate_config_rejects_unknown_thresholds_and_blocking_rules() {
    let mut unknown_threshold = thresholds();
    unknown_threshold.insert("vibes".to_owned(), 500);
    assert_eq!(
        QualityGateConfig::new("gate:quality", 1, sha('1'), unknown_threshold, rules()),
        Err("gate_config_threshold_unknown")
    );

    assert_eq!(
        QualityGateConfig::new(
            "gate:quality",
            1,
            sha('1'),
            thresholds(),
            vec!["vibes".to_owned()]
        ),
        Err("gate_config_blocking_rule_unknown")
    );

    // Rules must be sorted, unique, non-empty and bounded.
    assert_eq!(
        QualityGateConfig::new(
            "gate:quality",
            1,
            sha('1'),
            thresholds(),
            vec!["safety".to_owned(), "infra".to_owned()]
        ),
        Err("gate_config_header_invalid")
    );
    assert_eq!(
        QualityGateConfig::new(
            "gate:quality",
            1,
            sha('1'),
            thresholds(),
            vec!["safety".to_owned(), "safety".to_owned()]
        ),
        Err("gate_config_header_invalid")
    );
    assert_eq!(
        QualityGateConfig::new("gate:quality", 1, sha('1'), thresholds(), Vec::new()),
        Err("gate_config_header_invalid")
    );
    let mut too_many_rules = Vec::new();
    for index in 0..=MAX_GATE_BLOCKING_RULES {
        too_many_rules.push(format!("rule_{index:02}"));
    }
    assert_eq!(
        QualityGateConfig::new("gate:quality", 1, sha('1'), thresholds(), too_many_rules),
        Err("gate_config_header_invalid")
    );

    // Threshold count and value bounds fail closed before the vocabulary is read.
    let mut too_many_thresholds = BTreeMap::new();
    for index in 0..=MAX_GATE_THRESHOLDS {
        too_many_thresholds.insert(format!("unknown_{index:02}"), 100);
    }
    assert_eq!(
        QualityGateConfig::new("gate:quality", 1, sha('1'), too_many_thresholds, rules()),
        Err("gate_config_header_invalid")
    );
    assert_eq!(
        QualityGateConfig::new("gate:quality", 1, sha('1'), BTreeMap::new(), rules()),
        Err("gate_config_header_invalid")
    );
    let mut zero = thresholds();
    zero.insert("functional".to_owned(), 0);
    assert_eq!(
        QualityGateConfig::new("gate:quality", 1, sha('1'), zero, rules()),
        Err("gate_config_header_invalid")
    );
    let mut over_limit = thresholds();
    over_limit.insert("functional".to_owned(), MAX_GATE_SCORE_MILLI + 1);
    assert_eq!(
        QualityGateConfig::new("gate:quality", 1, sha('1'), over_limit, rules()),
        Err("gate_config_header_invalid")
    );

    // The closed vocabularies are exported so later steps share one contract.
    assert!(GATE_THRESHOLD_DIMENSIONS.contains(&"policy_safety"));
    assert!(GATE_BLOCKING_RULES.contains(&"fixture_integrity"));
    assert_eq!(config(1, &sha('1')).validate(), Ok(()));
}

#[test]
fn gate_config_digest_tamper_is_refused() {
    let valid = config(1, &sha('1'));

    let mut digest_tamper = valid.clone();
    digest_tamper.config_digest = sha('f');
    assert_eq!(
        config_reason(&digest_tamper),
        Some("gate_config_digest_mismatch")
    );

    let mut threshold_tamper = valid.clone();
    threshold_tamper
        .thresholds
        .insert("policy_safety".to_owned(), 1);
    assert_eq!(
        config_reason(&threshold_tamper),
        Some("gate_config_digest_mismatch")
    );

    // Re-sealing a rewritten threshold map is a new configuration identity, so the earlier
    // digest can never carry the rewrite.
    let mut rewritten = threshold_tamper.clone();
    rewritten.config_digest = rewritten.digest();
    assert_eq!(rewritten.validate(), Ok(()));
    assert_ne!(rewritten.config_digest, valid.config_digest);
    let mut replayed = rewritten.clone();
    replayed.config_digest = valid.config_digest.clone();
    assert_eq!(
        config_reason(&replayed),
        Some("gate_config_digest_mismatch")
    );

    let mut schema_drift = valid.clone();
    schema_drift.schema = "kiana.quality-gate-config.v1".to_owned();
    assert_eq!(
        config_reason(&schema_drift),
        Some("gate_config_header_invalid")
    );

    let mut version_zero = valid.clone();
    version_zero.version = 0;
    version_zero.config_digest = version_zero.digest();
    assert_eq!(
        config_reason(&version_zero),
        Some("gate_config_header_invalid")
    );

    let mut uppercase_digest = valid.clone();
    uppercase_digest.suite_digest = sha('1').to_uppercase();
    assert_eq!(
        config_reason(&uppercase_digest),
        Some("gate_config_header_invalid")
    );

    let mut whitespace_identity = valid.clone();
    whitespace_identity.gate_id = "gate quality".to_owned();
    assert_eq!(
        config_reason(&whitespace_identity),
        Some("gate_config_header_invalid")
    );

    let mut oversized_identity = valid.clone();
    oversized_identity.gate_id = "g".repeat(MAX_GATE_TEXT_BYTES + 1);
    assert_eq!(
        config_reason(&oversized_identity),
        Some("gate_config_header_invalid")
    );

    // Every tamper reaches the evaluator as a stable finding with its reject reason.
    let findings = evaluate(&input(digest_tamper, pass_decision(&valid)));
    assert_eq!(codes(&findings), vec!["gate.config_invalid"]);
    assert_eq!(findings[0].actual, json!("gate_config_digest_mismatch"));
    assert_eq!(findings[0].expected, json!("validated_immutable_config"));
    assert_eq!(findings[0].evidence_ref, "gate:input");
}

#[test]
fn gate_decision_verdict_matrix_is_enforced() {
    let v1 = config(1, &sha('1'));

    // A Pass may not carry blockers or known regressions.
    assert_eq!(
        QualityGateDecision::new(
            "decision:one",
            &v1,
            "candidate:one",
            sha('9'),
            GateVerdict::Pass,
            0,
            vec!["safety".to_owned()],
            Vec::new(),
            "evidence:run:one",
            41,
            1_000
        ),
        Err("gate_decision_verdict_inconsistent")
    );
    assert_eq!(
        QualityGateDecision::new(
            "decision:one",
            &v1,
            "candidate:one",
            sha('9'),
            GateVerdict::Pass,
            0,
            Vec::new(),
            vec!["replay".to_owned()],
            "evidence:run:one",
            41,
            1_000
        ),
        Err("gate_decision_verdict_inconsistent")
    );

    // Reject and Rollback need at least one reason.
    for verdict in [GateVerdict::Reject, GateVerdict::Rollback] {
        assert_eq!(
            QualityGateDecision::new(
                "decision:one",
                &v1,
                "candidate:one",
                sha('9'),
                verdict,
                0,
                Vec::new(),
                Vec::new(),
                "evidence:run:one",
                41,
                1_000
            ),
            Err("gate_decision_verdict_inconsistent")
        );
    }

    // NeedsShadow may not conceal a blocking finding, but needs no evidence list.
    assert_eq!(
        QualityGateDecision::new(
            "decision:one",
            &v1,
            "candidate:one",
            sha('9'),
            GateVerdict::NeedsShadow,
            0,
            vec!["safety".to_owned()],
            Vec::new(),
            "evidence:run:one",
            41,
            1_000
        ),
        Err("gate_decision_verdict_inconsistent")
    );
    assert!(QualityGateDecision::new(
        "decision:shadow",
        &v1,
        "candidate:one",
        sha('9'),
        GateVerdict::NeedsShadow,
        0,
        Vec::new(),
        Vec::new(),
        "evidence:run:one",
        41,
        1_000
    )
    .is_ok());
    assert!(QualityGateDecision::new(
        "decision:reject",
        &v1,
        "candidate:one",
        sha('9'),
        GateVerdict::Reject,
        0,
        vec!["safety".to_owned()],
        Vec::new(),
        "evidence:run:one",
        41,
        1_000
    )
    .is_ok());
    assert!(QualityGateDecision::new(
        "decision:safety_reversal",
        &v1,
        "candidate:one",
        sha('9'),
        GateVerdict::Rollback,
        0,
        Vec::new(),
        vec!["replay".to_owned()],
        "evidence:run:one",
        41,
        1_000
    )
    .is_ok());

    // A decoded Pass that smuggles blockers is refused by validation and reported as the card
    // named code rather than silently accepted.
    let mut blocked = pass_decision(&v1);
    blocked.blocking_findings = vec!["safety".to_owned()];
    reseal_decision(&mut blocked);
    assert_eq!(
        decision_reason(&blocked),
        Some("gate_decision_verdict_inconsistent")
    );
    let findings = evaluate(&input(v1.clone(), blocked));
    assert_eq!(codes(&findings), vec!["gate.pass_with_blocking_findings"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_verdict_inconsistent")
    );
    assert_eq!(findings[0].expected, json!("validated_immutable_decision"));

    // Every verdict keeps a stable key.
    assert_eq!(GateVerdict::ALL.len(), 4);
    for (verdict, key) in [
        (GateVerdict::Pass, "pass"),
        (GateVerdict::Reject, "reject"),
        (GateVerdict::NeedsShadow, "needs_shadow"),
        (GateVerdict::Rollback, "rollback"),
    ] {
        assert_eq!(verdict.key(), key);
    }

    assert!(evaluate(&input(v1, pass_decision(&config(1, &sha('1'))))).is_empty());
}

#[test]
fn gate_decision_digest_and_header_tamper_is_refused() {
    let v1 = config(1, &sha('1'));
    let decision = pass_decision(&v1);

    let mut digest_tamper = decision.clone();
    digest_tamper.decision_digest = sha('f');
    assert_eq!(
        decision_reason(&digest_tamper),
        Some("gate_decision_digest_mismatch")
    );

    let mut cursor_zero = decision.clone();
    cursor_zero.source_cursor = 0;
    reseal_decision(&mut cursor_zero);
    assert_eq!(
        decision_reason(&cursor_zero),
        Some("gate_decision_header_invalid")
    );

    let mut time_zero = decision.clone();
    time_zero.decided_at_unix_ms = 0;
    reseal_decision(&mut time_zero);
    assert_eq!(
        decision_reason(&time_zero),
        Some("gate_decision_header_invalid")
    );

    let mut version_zero = decision.clone();
    version_zero.gate_version = 0;
    reseal_decision(&mut version_zero);
    assert_eq!(
        decision_reason(&version_zero),
        Some("gate_decision_header_invalid")
    );

    let mut blank_evidence = decision.clone();
    blank_evidence.evidence_ref = " ".to_owned();
    reseal_decision(&mut blank_evidence);
    assert_eq!(
        decision_reason(&blank_evidence),
        Some("gate_decision_header_invalid")
    );

    let mut unsorted_reasons = decision.clone();
    unsorted_reasons.verdict = GateVerdict::Reject;
    // Deliberately out of lexicographic order: the contract requires sorted unique lists, so
    // `["safety", "replay"]` (not the sorted `["replay", "safety"]`) is the tampered value.
    unsorted_reasons.known_regressions = vec!["safety".to_owned(), "replay".to_owned()];
    reseal_decision(&mut unsorted_reasons);
    assert_eq!(
        decision_reason(&unsorted_reasons),
        Some("gate_decision_header_invalid")
    );

    let mut schema_drift = decision.clone();
    schema_drift.schema = "kiana.quality-gate-decision.v1".to_owned();
    assert_eq!(
        decision_reason(&schema_drift),
        Some("gate_decision_header_invalid")
    );

    let mut oversized_candidate = decision.clone();
    oversized_candidate.candidate_id = "c".repeat(MAX_GATE_TEXT_BYTES + 1);
    reseal_decision(&mut oversized_candidate);
    assert_eq!(
        decision_reason(&oversized_candidate),
        Some("gate_decision_header_invalid")
    );

    let mut too_many_blockers = decision.clone();
    too_many_blockers.verdict = GateVerdict::Reject;
    too_many_blockers.blocking_findings = (0..=MAX_GATE_EVIDENCE_REFS)
        .map(|index| format!("blocker_{index:03}"))
        .collect();
    reseal_decision(&mut too_many_blockers);
    assert_eq!(
        decision_reason(&too_many_blockers),
        Some("gate_decision_header_invalid")
    );

    let mut score_out_of_range = decision.clone();
    score_out_of_range.score_delta_milli = MAX_GATE_SCORE_MILLI as i64 + 1;
    reseal_decision(&mut score_out_of_range);
    assert_eq!(
        decision_reason(&score_out_of_range),
        Some("gate_decision_header_invalid")
    );

    let findings = evaluate(&input(v1, digest_tamper));
    assert_eq!(codes(&findings), vec!["gate.decision_invalid"]);
    assert_eq!(findings[0].actual, json!("gate_decision_digest_mismatch"));
}

#[test]
fn gate_decision_must_bind_the_exact_config_version() {
    let v1 = config(1, &sha('1'));
    let v2 = config(2, &sha('1'));
    let decision = pass_decision(&v1);

    // A decision made under v1 does not bind v2.
    let findings = evaluate(&input(v2.clone(), decision.clone()));
    assert_eq!(codes(&findings), vec!["gate.decision_config_mismatch"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_gate_version_mismatch")
    );
    assert_eq!(
        quality_gate_decision_binding(&v2, &decision),
        Err("gate_decision_gate_version_mismatch")
    );

    // Identity axis: another gate cannot adopt this decision.
    let other = QualityGateConfig::new("gate:other", 1, sha('1'), thresholds(), rules())
        .expect("other gate config");
    assert_eq!(
        quality_gate_decision_binding(&other, &decision),
        Err("gate_decision_config_identity_mismatch")
    );
    let findings = evaluate(&input(other, decision.clone()));
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_config_identity_mismatch")
    );

    // Digest axis: same gate identity and version, different config digest.
    let mut digest_drift = v1.clone();
    digest_drift
        .thresholds
        .insert("policy_safety".to_owned(), 999);
    digest_drift.config_digest = digest_drift.digest();
    assert_eq!(
        quality_gate_decision_binding(&digest_drift, &decision),
        Err("gate_decision_config_digest_mismatch")
    );
    let findings = evaluate(&input(digest_drift, decision.clone()));
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_config_digest_mismatch")
    );

    // Suite axis: a decision for another suite cannot cite this configuration.
    let mut other_suite = decision.clone();
    other_suite.suite_digest = sha('e');
    reseal_decision(&mut other_suite);
    assert_eq!(
        quality_gate_decision_binding(&v1, &other_suite),
        Err("gate_decision_suite_digest_mismatch")
    );
    let findings = evaluate(&input(v1.clone(), other_suite));
    assert_eq!(codes(&findings), vec!["gate.decision_config_mismatch"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_suite_digest_mismatch")
    );

    // The untouched pair binds and stays empty.
    assert_eq!(quality_gate_decision_binding(&v1, &decision), Ok(()));
    assert!(evaluate(&input(v1, decision)).is_empty());
}

#[test]
fn gate_update_requires_a_newer_distinct_version() {
    let v1 = config(1, &sha('1'));
    let v2 = config(2, &sha('2'));

    assert_eq!(quality_gate_config_update(&v1, &v2), Ok(()));
    assert_eq!(
        quality_gate_config_update(&v1, &v1),
        Err("gate_config_update_version_not_incremented")
    );

    // Same version with different content is a rewrite, not an update.
    let same_version = config(1, &sha('2'));
    assert_eq!(
        quality_gate_config_update(&v1, &same_version),
        Err("gate_config_update_version_not_incremented")
    );
    // An older version is refused even when its digest is distinct.
    assert_eq!(
        quality_gate_config_update(&v2, &v1),
        Err("gate_config_update_version_not_incremented")
    );

    // Another gate identity is refused.
    let other = QualityGateConfig::new("gate:other", 2, sha('2'), thresholds(), rules())
        .expect("other gate config");
    assert_eq!(
        quality_gate_config_update(&v1, &other),
        Err("gate_config_update_identity_mismatch")
    );

    // An invalid current version is refused before the update is judged.
    let mut broken = v1.clone();
    broken.config_digest = sha('f');
    assert_eq!(
        quality_gate_config_update(&broken, &v2),
        Err("gate_config_update_current_invalid")
    );

    // The evaluator reports the axis, and an invalid replacement is reported first.
    let findings = evaluate(
        &GateInput::new(v1.clone(), pass_decision(&v1))
            .with_update(same_version)
            .as_value(),
    );
    assert_eq!(codes(&findings), vec!["gate.config_update_invalid"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_config_update_version_not_incremented")
    );

    let mut invalid_update = v2.clone();
    invalid_update.config_digest = sha('f');
    let findings = evaluate(
        &GateInput::new(v1.clone(), pass_decision(&v1))
            .with_update(invalid_update)
            .as_value(),
    );
    assert_eq!(codes(&findings), vec!["gate.updated_config_invalid"]);
    assert_eq!(findings[0].actual, json!("gate_config_digest_mismatch"));

    // A next version with identical thresholds still has a distinct digest, because the version
    // is part of the digested content.
    let same_content = QualityGateConfig::new("gate:quality", 2, sha('1'), thresholds(), rules())
        .expect("same content next version");
    assert_eq!(quality_gate_config_update(&v1, &same_content), Ok(()));
    assert_ne!(same_content.config_digest, v1.config_digest);
    assert_eq!(same_content.thresholds, v1.thresholds);
}

#[test]
fn gate_decision_log_is_append_only_and_replay_safe() {
    let v1 = config(1, &sha('1'));
    let v2 = config(2, &sha('2'));
    let first = pass_decision(&v1);
    let mut log = QualityGateDecisionLog::new("gate:quality");
    let empty_digest = log.log_digest.clone();
    assert_eq!(log.append(first.clone()), Ok(true));
    assert_eq!(log.decisions.len(), 1);
    assert_ne!(log.log_digest, empty_digest);

    // The exact same decision is a replay that mutates nothing.
    let after_first = log.log_digest.clone();
    assert_eq!(log.append(first.clone()), Ok(false));
    assert_eq!(log.decisions.len(), 1);
    assert_eq!(log.log_digest, after_first);

    // A second decision under the next config version appends.
    let second = QualityGateDecision::new(
        "decision:two",
        &v2,
        "candidate:two",
        sha('7'),
        GateVerdict::Reject,
        0,
        vec!["safety".to_owned()],
        Vec::new(),
        "evidence:run:two",
        42,
        2_000,
    )
    .expect("second decision");
    assert_eq!(log.append(second), Ok(true));
    assert_eq!(log.decisions.len(), 2);
    assert_eq!(log.latest().map(|decision| decision.gate_version), Some(2));
    assert_eq!(
        log.find("decision:one")
            .map(|decision| decision.gate_version),
        Some(1)
    );

    // The history replays a published version without mutating, and refuses a rewrite.
    let mut history = GateConfigHistory::new("gate:quality");
    assert_eq!(history.append(v1.clone()), Ok(true));
    assert_eq!(history.append(v2.clone()), Ok(true));
    let history_digest = history.history_digest.clone();
    assert_eq!(history.append(v1.clone()), Ok(false));
    assert_eq!(history.history_digest, history_digest);
    let mut rewritten = v1.clone();
    rewritten.thresholds.insert("policy_safety".to_owned(), 999);
    rewritten.config_digest = rewritten.digest();
    assert_eq!(
        history.append(rewritten),
        Err("gate_config_version_rewritten")
    );
    assert_eq!(
        history.find(1).map(|config| config.config_digest.clone()),
        Some(v1.config_digest.clone())
    );
    assert_eq!(history.latest().map(|config| config.version), Some(2));
    assert_eq!(history_reason(&history), None);

    // The sealed pair passes through the log evaluator.
    assert!(evaluate_log(&log_input(history, log)).is_empty());
}

#[test]
fn gate_decision_log_refuses_a_rewritten_decision() {
    let v1 = config(1, &sha('1'));
    let decision = reject_decision(&v1, "decision:one", "candidate:one", 41);
    let mut log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(log.append(decision.clone()), Ok(true));
    let sealed_digest = log.log_digest.clone();

    // Same decision_id, different content: refused and nothing is mutated.
    let mut flipped = decision.clone();
    flipped.verdict = GateVerdict::NeedsShadow;
    flipped.blocking_findings = Vec::new();
    reseal_decision(&mut flipped);
    assert_eq!(flipped.validate(), Ok(()));
    assert_ne!(flipped.decision_digest, decision.decision_digest);
    assert_eq!(log.append(flipped.clone()), Err("gate_decision_rewritten"));
    assert_eq!(log.log_digest, sealed_digest);
    assert_eq!(
        log.find("decision:one")
            .map(|decision| decision.decision_digest.clone()),
        Some(decision.decision_digest.clone())
    );

    // A decoded log that carries the rewrite is refused with the same law.
    let mut tampered_log = log.clone();
    tampered_log.decisions.push(flipped);
    reseal_log(&mut tampered_log);
    assert_eq!(log_reason(&tampered_log), Some("gate_decision_rewritten"));

    // A tampered log digest without any rewrite is a digest refusal.
    let mut digest_tamper = log.clone();
    digest_tamper.log_digest = sha('f');
    assert_eq!(
        log_reason(&digest_tamper),
        Some("gate_decision_log_digest_mismatch")
    );

    let mut history = GateConfigHistory::new("gate:quality");
    assert_eq!(history.append(v1.clone()), Ok(true));
    assert_eq!(history.append(config(2, &sha('2'))), Ok(true));
    let findings = evaluate_log(&log_input(history.clone(), tampered_log));
    assert_eq!(codes(&findings), vec!["gate.decision_rewritten"]);
    assert_eq!(findings[0].actual, json!("gate_decision_rewritten"));
    assert_eq!(findings[0].expected, json!("append_only_decision_log"));
    assert_eq!(findings[0].evidence_ref, "gate:log");

    // The untouched log stays valid.
    assert!(evaluate_log(&log_input(history, log)).is_empty());
}

#[test]
fn gate_decision_log_refuses_redeciding_a_candidate_under_the_same_config() {
    let v1 = config(1, &sha('1'));
    let v2 = config(2, &sha('2'));
    let first = pass_decision(&v1);
    let mut log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(log.append(first.clone()), Ok(true));

    // A second decision about the same candidate under the same config version is refused.
    let redecided = QualityGateDecision::new(
        "decision:two",
        &v1,
        "candidate:one",
        sha('9'),
        GateVerdict::Reject,
        0,
        vec!["safety".to_owned()],
        Vec::new(),
        "evidence:run:two",
        42,
        2_000,
    )
    .expect("redecision under v1");
    assert_eq!(
        log.append(redecided.clone()),
        Err("gate_decision_candidate_redecided")
    );
    assert_eq!(log.decisions.len(), 1);

    // A different candidate under the same config version appends normally.
    let other_candidate = QualityGateDecision::new(
        "decision:three",
        &v1,
        "candidate:two",
        sha('7'),
        GateVerdict::Reject,
        0,
        vec!["safety".to_owned()],
        Vec::new(),
        "evidence:run:three",
        43,
        3_000,
    )
    .expect("other candidate");
    assert_eq!(log.append(other_candidate), Ok(true));

    // The same candidate under a newer config version is a new decision.
    let under_v2 = QualityGateDecision::new(
        "decision:four",
        &v2,
        "candidate:one",
        sha('9'),
        GateVerdict::Reject,
        0,
        vec!["safety".to_owned()],
        Vec::new(),
        "evidence:run:four",
        44,
        4_000,
    )
    .expect("redecision under v2");
    assert_eq!(log.append(under_v2), Ok(true));
    assert_eq!(log.decisions.len(), 3);

    // A decoded log that re-decides the candidate is refused on the same law.
    let mut history = GateConfigHistory::new("gate:quality");
    assert_eq!(history.append(v1.clone()), Ok(true));
    assert_eq!(history.append(v2.clone()), Ok(true));

    let mut redecided_log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(redecided_log.append(first.clone()), Ok(true));
    redecided_log.decisions.push(redecided);
    reseal_log(&mut redecided_log);
    assert_eq!(
        log_reason(&redecided_log),
        Some("gate_decision_candidate_redecided")
    );
    let findings = evaluate_log(&log_input(history.clone(), redecided_log));
    assert_eq!(codes(&findings), vec!["gate.candidate_redecided"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_candidate_redecided")
    );

    assert!(evaluate_log(&log_input(history, log)).is_empty());
}

#[test]
fn gate_decision_log_refuses_a_config_version_regression() {
    let v1 = config(1, &sha('1'));
    let v2 = config(2, &sha('2'));
    let under_v2 = reject_decision(&v2, "decision:one", "candidate:one", 42);
    let mut log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(log.append(under_v2.clone()), Ok(true));

    // An older config version cannot be appended after a newer one.
    let under_v1 = reject_decision(&v1, "decision:two", "candidate:two", 43);
    assert_eq!(
        log.append(under_v1.clone()),
        Err("gate_decision_log_version_regressed")
    );
    assert_eq!(log.decisions.len(), 1);

    // The cursor and the decision time may not regress either.
    let cursor_regression = reject_decision(&v2, "decision:three", "candidate:three", 42);
    assert_eq!(
        log.append(cursor_regression),
        Err("gate_decision_log_cursor_regressed")
    );
    let mut time_regression = reject_decision(&v2, "decision:four", "candidate:four", 44);
    time_regression.decided_at_unix_ms = 1_999;
    reseal_decision(&mut time_regression);
    assert_eq!(
        log.append(time_regression),
        Err("gate_decision_log_time_regressed")
    );

    // A decoded log with a regression is refused with the mapped finding code.
    let mut history = GateConfigHistory::new("gate:quality");
    assert_eq!(history.append(v1.clone()), Ok(true));
    assert_eq!(history.append(v2.clone()), Ok(true));
    let mut regressed = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(regressed.append(under_v2), Ok(true));
    regressed.decisions.push(under_v1);
    reseal_log(&mut regressed);
    assert_eq!(
        log_reason(&regressed),
        Some("gate_decision_log_version_regressed")
    );
    let findings = evaluate_log(&log_input(history.clone(), regressed));
    assert_eq!(codes(&findings), vec!["gate.config_version_regressed"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_log_version_regressed")
    );

    // The untouched log stays valid.
    assert!(evaluate_log(&log_input(history, log)).is_empty());
}

#[test]
fn gate_log_entries_must_bind_the_published_config_history() {
    let v1 = config(1, &sha('1'));
    let v2 = config(2, &sha('2'));
    let mut history = GateConfigHistory::new("gate:quality");
    assert_eq!(history.append(v1.clone()), Ok(true));

    // A decision that names a version the history never published is refused.
    let ahead = reject_decision(&v2, "decision:one", "candidate:one", 42);
    let mut ahead_log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(ahead_log.append(ahead), Ok(true));
    let findings = evaluate_log(&log_input(history.clone(), ahead_log));
    assert_eq!(codes(&findings), vec!["gate.log_entry_version_ahead"]);
    assert_eq!(findings[0].actual, json!("version"));

    // A decision that cites a rewritten digest for a published version is refused, and the
    // history itself refuses to publish that rewrite.
    let mut rewrite_config = v1.clone();
    rewrite_config
        .thresholds
        .insert("policy_safety".to_owned(), 999);
    rewrite_config.config_digest = rewrite_config.digest();
    let mut rewritten = pass_decision(&v1);
    rewritten.config_digest = rewrite_config.config_digest.clone();
    reseal_decision(&mut rewritten);
    let mut rewritten_log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(rewritten_log.append(rewritten), Ok(true));
    let findings = evaluate_log(&log_input(history.clone(), rewritten_log));
    assert_eq!(codes(&findings), vec!["gate.log_entry_config_mismatch"]);
    assert_eq!(
        findings[0].actual,
        json!("gate_decision_config_digest_mismatch")
    );
    assert_eq!(
        history.append(rewrite_config),
        Err("gate_config_version_rewritten")
    );
    assert_eq!(history_reason(&history), None);

    // Identity drift between the history and the log is refused.
    let mut other_history = GateConfigHistory::new("gate:other");
    assert_eq!(
        other_history.append(
            QualityGateConfig::new("gate:other", 1, sha('1'), thresholds(), rules())
                .expect("other gate config")
        ),
        Ok(true)
    );
    let mut log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(log.append(pass_decision(&v1)), Ok(true));
    let findings = evaluate_log(&log_input(other_history, log.clone()));
    assert_eq!(codes(&findings), vec!["gate.log_identity_mismatch"]);

    // An invalid entry inside the log is reported before the history binding is read.
    let mut broken_entry = log.clone();
    broken_entry.decisions[0].decision_digest = sha('f');
    reseal_log(&mut broken_entry);
    let findings = evaluate_log(&log_input(history.clone(), broken_entry));
    assert_eq!(codes(&findings), vec!["gate.log_invalid"]);
    assert_eq!(findings[0].actual, json!("gate_decision_log_entry_invalid"));

    // A history that skips a version is refused as a history.
    let mut gapped = GateConfigHistory::new("gate:quality");
    assert_eq!(gapped.append(v1.clone()), Ok(true));
    gapped.versions.push(config(3, &sha('3')));
    let value = serde_json::to_value(&gapped).expect("gapped history value");
    let findings = evaluate_log(&log_input(
        serde_json::from_value(value).expect("gapped history decode"),
        log.clone(),
    ));
    assert_eq!(codes(&findings), vec!["gate.config_history_invalid"]);
    assert_eq!(findings[0].actual, json!("gate_config_version_gap"));
    assert_eq!(findings[0].expected, json!("append_only_config_history"));

    // Envelope and unknown-field strictness.
    let mut envelope_schema = log_input(history.clone(), log.clone());
    envelope_schema["schema"] = json!("kiana.quality-gate-log-input.v2");
    assert_eq!(
        decoded_log_input(&envelope_schema).validate(),
        Err("gate_log_input_schema_invalid")
    );
    assert!(GateLogEvaluator.evaluate(&envelope_schema).is_err());
    let mut envelope_unknown = log_input(history.clone(), log.clone());
    envelope_unknown["extra"] = json!(true);
    assert!(GateLogEvaluator.evaluate(&envelope_unknown).is_err());
    let mut decision_unknown = log_input(history.clone(), log.clone());
    decision_unknown["log"]["decisions"][0]["hidden"] = json!("x");
    assert!(GateLogEvaluator.evaluate(&decision_unknown).is_err());
    let mut config_unknown = log_input(history.clone(), log.clone());
    config_unknown["config_history"]["versions"][0]["verdict"] = json!("pass");
    assert!(GateLogEvaluator.evaluate(&config_unknown).is_err());

    // The untouched pair passes.
    assert!(evaluate_log(&log_input(history, log)).is_empty());
}

#[test]
fn gate_contracts_round_trip_and_are_registry_wired() {
    let v1 = config(1, &sha('1'));
    let v2 = config(2, &sha('1'));
    let decision = pass_decision(&v1);

    let value = serde_json::to_value(&v1).expect("config value");
    let decoded_config: QualityGateConfig =
        serde_json::from_value(value.clone()).expect("config decode");
    assert_eq!(decoded_config, v1);
    assert_eq!(decoded_config.digest(), v1.config_digest);

    let value = serde_json::to_value(&decision).expect("decision value");
    let decoded_decision: QualityGateDecision =
        serde_json::from_value(value.clone()).expect("decision decode");
    assert_eq!(decoded_decision, decision);
    assert_eq!(decoded_decision.digest(), decision.decision_digest);

    let gate_value = GateInput::new(v1.clone(), decision.clone())
        .with_update(v2.clone())
        .as_value();
    assert_eq!(gate_value["schema"], json!(GATE_INPUT_SCHEMA));
    assert_eq!(
        decoded_input(&gate_value),
        GateInput::new(v1.clone(), decision.clone()).with_update(v2.clone())
    );
    let mut envelope_schema = gate_value.clone();
    envelope_schema["schema"] = json!("kiana.quality-gate-input.v1");
    assert_eq!(
        decoded_input(&envelope_schema).validate(),
        Err("gate_input_schema_invalid")
    );
    assert!(GateEvaluator.evaluate(&envelope_schema).is_err());
    assert!(evaluate(&gate_value).is_empty());
    assert!(evaluate_gate(&decoded_input(&gate_value))
        .expect("direct gate evaluation")
        .is_empty());

    let mut history = GateConfigHistory::new("gate:quality");
    assert_eq!(history.append(v1.clone()), Ok(true));
    assert_eq!(history.append(v2.clone()), Ok(true));
    let history_value = serde_json::to_value(&history).expect("history value");
    let decoded_history: GateConfigHistory =
        serde_json::from_value(history_value).expect("history decode");
    assert_eq!(decoded_history, history);
    assert_eq!(
        decoded_history.latest().map(|config| config.version),
        Some(2)
    );

    let mut log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(log.append(decision.clone()), Ok(true));
    let value = serde_json::to_value(&log).expect("log value");
    let decoded_log: QualityGateDecisionLog = serde_json::from_value(value).expect("log decode");
    assert_eq!(decoded_log, log);

    // A rewritten decision reaches the direct call and the registry with the same findings, and
    // repeated evaluation is deterministic.
    let mut rewritten_log = log.clone();
    let mut flipped = decision.clone();
    flipped.verdict = GateVerdict::NeedsShadow;
    flipped.blocking_findings = Vec::new();
    reseal_decision(&mut flipped);
    rewritten_log.decisions.push(flipped);
    reseal_log(&mut rewritten_log);
    let log_value = log_input(history.clone(), rewritten_log);
    assert_eq!(log_value["schema"], json!(GATE_LOG_INPUT_SCHEMA));
    let direct = evaluate_log(&log_value);
    assert_eq!(codes(&direct), vec!["gate.decision_rewritten"]);
    assert_eq!(direct, evaluate_log(&log_value));
    assert_eq!(
        direct,
        evaluate_gate_log(&decoded_log_input(&log_value)).expect("direct log evaluation")
    );

    let mut registry = EvaluatorRegistry::new();
    registry
        .register(GateEvaluator)
        .expect("register gate evaluator");
    registry
        .register(GateLogEvaluator)
        .expect("register gate log evaluator");
    assert_eq!(
        registry.evaluator_ids(),
        vec![
            GATE_EVALUATOR_ID.to_owned(),
            GATE_LOG_EVALUATOR_ID.to_owned()
        ]
    );

    let mut log_registry = EvaluatorRegistry::new();
    log_registry
        .register(GateLogEvaluator)
        .expect("register gate log evaluator");
    assert_eq!(
        canonical_findings(
            log_registry
                .run(&log_value)
                .expect("registry log evaluation")
        )
        .expect("registry findings"),
        canonical_findings(direct).expect("direct findings")
    );

    let mut clean_log = QualityGateDecisionLog::new("gate:quality");
    assert_eq!(clean_log.append(decision), Ok(true));
    let clean_value = log_input(history, clean_log);
    assert!(evaluate_log(&clean_value).is_empty());
    assert!(log_registry
        .run(&clean_value)
        .expect("registry log evaluation")
        .is_empty());

    let mut gate_registry = EvaluatorRegistry::new();
    gate_registry
        .register(GateEvaluator)
        .expect("register gate evaluator");
    assert!(gate_registry
        .run(&gate_value)
        .expect("registry gate evaluation")
        .is_empty());
}
