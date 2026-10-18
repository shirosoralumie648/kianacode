use kiana_quality::{
    canonical_findings, BaselineRecord, CandidateAdmissionEvaluator, CandidateAdmissionInput,
    CandidateDimension, CandidateEvaluator, CandidateInput, CandidateStatus,
    DeterministicEvaluator, EvaluatorRegistry, Finding, QualityCandidate,
    CANDIDATE_ADMISSION_EVALUATOR_ID, CANDIDATE_ADMISSION_INPUT_SCHEMA, CANDIDATE_EVALUATOR_ID,
    CANDIDATE_INPUT_SCHEMA, CANDIDATE_SCHEMA, MAX_CANDIDATE_DIMENSIONS, MAX_CANDIDATE_TEXT_BYTES,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn sha(hex: char) -> String {
    format!("sha256:{}", hex.to_string().repeat(64))
}

fn baseline_record() -> BaselineRecord {
    BaselineRecord::new(
        "baseline:one",
        sha('a'),
        sha('b'),
        sha('c'),
        sha('d'),
        "quality-owner",
        100,
        200,
        "capture:one",
    )
}

fn other_record() -> BaselineRecord {
    BaselineRecord::new(
        "baseline:two",
        sha('a'),
        sha('b'),
        sha('c'),
        sha('d'),
        "quality-owner",
        100,
        200,
        "capture:two",
    )
}

fn base_versions() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("model".to_owned(), sha('1')),
        ("prompt".to_owned(), sha('2')),
        ("tool_catalog".to_owned(), sha('3')),
        ("memory_index".to_owned(), sha('4')),
        ("workflow".to_owned(), sha('5')),
        ("route".to_owned(), sha('6')),
    ])
}

fn candidate(record: &BaselineRecord) -> QualityCandidate {
    QualityCandidate::new(
        "candidate:one",
        record.baseline_id.as_str(),
        record.baseline_digest.as_str(),
        record.suite_digest.as_str(),
        CandidateDimension::Prompt,
        base_versions(),
        sha('9'),
        "quality-owner",
    )
    .expect("single-dimension candidate")
}

/// Recompute the snapshot and candidate digests after a deliberate edit.
fn seal(candidate: &mut QualityCandidate) {
    candidate.snapshot_digest = QualityCandidate::snapshot_digest(&candidate.snapshot_versions);
    candidate.candidate_digest = candidate.digest();
}

fn mutated(
    candidate: &QualityCandidate,
    apply: impl FnOnce(&mut QualityCandidate),
) -> QualityCandidate {
    let mut candidate = candidate.clone();
    apply(&mut candidate);
    seal(&mut candidate);
    candidate
}

fn input(candidate: QualityCandidate) -> Value {
    CandidateInput::new(candidate).as_value()
}

fn admission(record: &BaselineRecord, now_unix_ms: u64, candidate: QualityCandidate) -> Value {
    CandidateAdmissionInput::new(now_unix_ms, record.clone(), candidate).as_value()
}

fn decoded(value: &Value) -> CandidateInput {
    serde_json::from_value(value.clone()).expect("candidate input envelope")
}

fn decoded_admission(value: &Value) -> CandidateAdmissionInput {
    serde_json::from_value(value.clone()).expect("candidate admission envelope")
}

fn evaluate(value: &Value) -> Vec<Finding> {
    CandidateEvaluator
        .evaluate(value)
        .expect("candidate evaluation")
}

fn admit(value: &Value) -> Vec<Finding> {
    CandidateAdmissionEvaluator
        .evaluate(value)
        .expect("candidate admission evaluation")
}

fn codes(findings: &[Finding]) -> Vec<&str> {
    findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

fn reject_reason(candidate: &QualityCandidate) -> Option<&'static str> {
    candidate.validate().err()
}

#[test]
fn candidate_cannot_hide_model_or_prompt_drift() {
    let record = baseline_record();
    let candidate = candidate(&record);
    assert!(candidate.validate().is_ok());

    // A hidden model change only in the candidate snapshot contradicts the passive reference.
    let snapshot_only = mutated(&candidate, |candidate| {
        candidate
            .snapshot_versions
            .insert("model".to_owned(), sha('7'));
    });
    assert_eq!(
        reject_reason(&snapshot_only),
        Some("candidate_passive_version_drift")
    );

    // Hiding the prompt change behind an explicit passive reference is a primary-dimension
    // violation, even when the coverage count stays intact.
    let hidden_primary = mutated(&candidate, |candidate| {
        candidate.passive_version_refs.remove("model");
        candidate
            .passive_version_refs
            .insert("prompt".to_owned(), sha('2'));
    });
    assert_eq!(
        reject_reason(&hidden_primary),
        Some("candidate_primary_dimension_invalid")
    );

    // Rewriting every map and re-sealing is internally consistent, but it produces a different
    // candidate identity: the earlier digest can never carry the rewritten snapshot.
    let rewritten = mutated(&candidate, |candidate| {
        for map in [
            &mut candidate.base_versions,
            &mut candidate.passive_version_refs,
            &mut candidate.snapshot_versions,
        ] {
            map.insert("model".to_owned(), sha('7'));
        }
    });
    assert!(rewritten.validate().is_ok());
    assert_ne!(rewritten.candidate_digest, candidate.candidate_digest);
    let mut replayed = rewritten.clone();
    replayed.candidate_digest = candidate.candidate_digest.clone();
    assert_eq!(reject_reason(&replayed), Some("candidate_digest_mismatch"));

    for (value, reason) in [
        (input(snapshot_only), "candidate_passive_version_drift"),
        (input(hidden_primary), "candidate_primary_dimension_invalid"),
        (input(replayed), "candidate_digest_mismatch"),
    ] {
        let findings = evaluate(&value);
        assert_eq!(codes(&findings), vec!["candidate.binding_invalid"]);
        assert_eq!(findings[0].actual, json!(reason));
    }

    // The redefined candidate is a new, admissible candidate bound to the same baseline.
    assert!(evaluate(&input(rewritten.clone())).is_empty());
    assert!(admit(&admission(&record, 150, rewritten)).is_empty());
}

#[test]
fn passive_version_references_must_cover_every_unchanged_dimension() {
    let record = baseline_record();
    let candidate = candidate(&record);

    // Dropping a passive reference breaks the exact coverage count.
    let missing = mutated(&candidate, |candidate| {
        candidate.passive_version_refs.remove("model");
    });
    assert_eq!(reject_reason(&missing), Some("candidate_header_invalid"));

    // A passive reference that claims another version is drift.
    let drifted = mutated(&candidate, |candidate| {
        candidate
            .passive_version_refs
            .insert("tool_catalog".to_owned(), sha('8'));
    });
    assert_eq!(
        reject_reason(&drifted),
        Some("candidate_passive_version_drift")
    );

    // An extra passive reference breaks the coverage count.
    let extra = mutated(&candidate, |candidate| {
        candidate
            .passive_version_refs
            .insert("route_extra".to_owned(), sha('7'));
    });
    assert_eq!(reject_reason(&extra), Some("candidate_header_invalid"));

    // A foreign key that keeps the count is refused as an unknown dimension.
    let unknown = mutated(&candidate, |candidate| {
        candidate.passive_version_refs.remove("route");
        candidate
            .passive_version_refs
            .insert("hidden".to_owned(), sha('7'));
    });
    assert_eq!(reject_reason(&unknown), Some("candidate_dimension_unknown"));
}

#[test]
fn a_second_changed_dimension_is_refused() {
    let record = baseline_record();
    let candidate = candidate(&record);

    // The snapshot changes the tool catalog as well as the primary prompt dimension.
    let second_change = mutated(&candidate, |candidate| {
        candidate
            .snapshot_versions
            .insert("tool_catalog".to_owned(), sha('8'));
    });
    assert_eq!(
        reject_reason(&second_change),
        Some("candidate_passive_version_drift")
    );

    // A maintained two-dimension change is a different candidate: both the snapshot digest and
    // the candidate digest move, so the single primary dimension cannot be re-labelled silently.
    let relabelled = mutated(&candidate, |candidate| {
        for map in [
            &mut candidate.base_versions,
            &mut candidate.passive_version_refs,
            &mut candidate.snapshot_versions,
        ] {
            map.insert("tool_catalog".to_owned(), sha('8'));
        }
    });
    assert!(relabelled.validate().is_ok());
    assert_eq!(relabelled.changed_dimension, CandidateDimension::Prompt);
    assert_ne!(relabelled.snapshot_digest, candidate.snapshot_digest);
    assert_ne!(relabelled.candidate_digest, candidate.candidate_digest);

    // The constructor itself refuses to build a candidate without a real primary change.
    assert_eq!(
        QualityCandidate::new(
            "candidate:one",
            record.baseline_id.as_str(),
            record.baseline_digest.as_str(),
            record.suite_digest.as_str(),
            CandidateDimension::Prompt,
            base_versions(),
            sha('2'),
            "quality-owner",
        ),
        Err("candidate_changed_dimension_unchanged")
    );
}

#[test]
fn unknown_dimensions_and_no_op_changes_are_refused() {
    let record = baseline_record();
    let candidate = candidate(&record);

    // Only the six known dimensions may appear in a version map.
    let renamed = mutated(&candidate, |candidate| {
        for map in [
            &mut candidate.base_versions,
            &mut candidate.passive_version_refs,
            &mut candidate.snapshot_versions,
        ] {
            map.remove("route");
            map.insert("hidden".to_owned(), sha('6'));
        }
    });
    assert_eq!(reject_reason(&renamed), Some("candidate_dimension_unknown"));

    // The primary dimension must exist in the baseline versions.
    assert_eq!(
        QualityCandidate::new(
            "candidate:one",
            record.baseline_id.as_str(),
            record.baseline_digest.as_str(),
            record.suite_digest.as_str(),
            CandidateDimension::Route,
            BTreeMap::from([
                ("model".to_owned(), sha('1')),
                ("prompt".to_owned(), sha('2')),
            ]),
            sha('9'),
            "quality-owner",
        ),
        Err("candidate_changed_dimension_missing")
    );

    // A decoded candidate that keeps the version counts but drops the primary dimension is
    // refused at validation as well.
    let missing_primary = mutated(&candidate, |candidate| {
        candidate.base_versions.remove("prompt");
        candidate.snapshot_versions.remove("prompt");
        candidate.passive_version_refs.remove("model");
    });
    assert_eq!(
        reject_reason(&missing_primary),
        Some("candidate_changed_dimension_missing")
    );

    // A decoded no-op candidate is refused the same way as the constructor.
    let no_op = mutated(&candidate, |candidate| {
        candidate.changed_version_ref = sha('2');
        candidate
            .snapshot_versions
            .insert("prompt".to_owned(), sha('2'));
    });
    assert_eq!(
        reject_reason(&no_op),
        Some("candidate_changed_dimension_unchanged")
    );
}

#[test]
fn snapshot_or_candidate_digest_tamper_is_rejected() {
    let record = baseline_record();
    let candidate = candidate(&record);

    let mut snapshot_tamper = candidate.clone();
    snapshot_tamper.snapshot_digest = sha('f');
    assert_eq!(
        reject_reason(&snapshot_tamper),
        Some("candidate_snapshot_digest_mismatch")
    );

    let mut candidate_tamper = candidate.clone();
    candidate_tamper.candidate_digest = sha('f');
    assert_eq!(
        reject_reason(&candidate_tamper),
        Some("candidate_digest_mismatch")
    );

    let mut schema_drift = candidate.clone();
    schema_drift.schema = "kiana.quality-candidate.v1".to_owned();
    assert_eq!(
        reject_reason(&schema_drift),
        Some("candidate_header_invalid")
    );

    let mut uppercase_digest = candidate.clone();
    uppercase_digest.changed_version_ref = sha('9').to_uppercase();
    assert_eq!(
        reject_reason(&uppercase_digest),
        Some("candidate_header_invalid")
    );

    let mut whitespace_id = candidate.clone();
    whitespace_id.candidate_id = "candidate one".to_owned();
    assert_eq!(
        reject_reason(&whitespace_id),
        Some("candidate_header_invalid")
    );

    let mut oversized_owner = candidate.clone();
    oversized_owner.owner_id = "o".repeat(MAX_CANDIDATE_TEXT_BYTES + 1);
    assert_eq!(
        reject_reason(&oversized_owner),
        Some("candidate_header_invalid")
    );

    let mut empty_versions = candidate.clone();
    empty_versions.base_versions.clear();
    empty_versions.passive_version_refs.clear();
    empty_versions.snapshot_versions.clear();
    assert_eq!(
        reject_reason(&empty_versions),
        Some("candidate_header_invalid")
    );

    // Every tamper reaches the evaluator as the stable finding code plus the reject reason.
    let findings = evaluate(&input(snapshot_tamper));
    assert_eq!(codes(&findings), vec!["candidate.binding_invalid"]);
    assert_eq!(
        findings[0].actual,
        json!("candidate_snapshot_digest_mismatch")
    );
    assert_eq!(
        findings[0].expected,
        json!("valid_single_dimension_candidate")
    );
    assert_eq!(findings[0].evidence_ref, "candidate:input");
}

#[test]
fn rewriting_the_version_snapshot_changes_the_candidate_identity() {
    let record = baseline_record();
    let candidate = candidate(&record);
    let first_digest = candidate.candidate_digest.clone();

    let mut changed_primary = candidate.clone();
    changed_primary.changed_version_ref = sha('8');
    changed_primary
        .snapshot_versions
        .insert("prompt".to_owned(), sha('8'));
    seal(&mut changed_primary);
    assert!(changed_primary.validate().is_ok());
    assert_ne!(changed_primary.candidate_digest, first_digest);
    assert_ne!(changed_primary.snapshot_digest, candidate.snapshot_digest);

    let mut changed_base = candidate.clone();
    for map in [
        &mut changed_base.base_versions,
        &mut changed_base.passive_version_refs,
        &mut changed_base.snapshot_versions,
    ] {
        map.insert("model".to_owned(), sha('7'));
    }
    seal(&mut changed_base);
    assert!(changed_base.validate().is_ok());
    assert_ne!(changed_base.candidate_digest, first_digest);

    // An earlier identity cannot carry a rewritten snapshot, and a digest from another candidate
    // cannot be reused as this candidate's snapshot digest.
    let mut replayed_identity = changed_primary.clone();
    replayed_identity.candidate_digest = first_digest.clone();
    assert_eq!(
        reject_reason(&replayed_identity),
        Some("candidate_digest_mismatch")
    );

    let mut replayed_snapshot = changed_base.clone();
    replayed_snapshot.snapshot_digest = candidate.snapshot_digest.clone();
    assert_eq!(
        reject_reason(&replayed_snapshot),
        Some("candidate_snapshot_digest_mismatch")
    );
}

#[test]
fn candidate_envelopes_and_unknown_fields_fail_closed() {
    let record = baseline_record();
    let candidate = candidate(&record);

    let mut envelope_schema = input(candidate.clone());
    envelope_schema["schema"] = json!("kiana.quality-candidate-input.v1");
    assert_eq!(
        decoded(&envelope_schema).validate(),
        Err("candidate_input_schema_invalid")
    );
    assert!(CandidateEvaluator.evaluate(&envelope_schema).is_err());

    let mut envelope_unknown = input(candidate.clone());
    envelope_unknown["extra"] = json!(true);
    assert!(CandidateEvaluator.evaluate(&envelope_unknown).is_err());

    let mut candidate_unknown = input(candidate.clone());
    candidate_unknown["candidate"]["hidden_versions"] = json!({"model": sha('1')});
    assert!(CandidateEvaluator.evaluate(&candidate_unknown).is_err());

    let mut admission_schema = admission(&record, 150, candidate.clone());
    admission_schema["schema"] = json!("kiana.quality-candidate-admission-input.v2");
    assert_eq!(
        decoded_admission(&admission_schema).validate(),
        Err("candidate_admission_input_schema_invalid")
    );
    assert!(CandidateAdmissionEvaluator
        .evaluate(&admission_schema)
        .is_err());

    let admission_time = admission(&record, 0, candidate.clone());
    assert_eq!(
        decoded_admission(&admission_time).validate(),
        Err("candidate_admission_input_time_invalid")
    );
    assert!(CandidateAdmissionEvaluator
        .evaluate(&admission_time)
        .is_err());

    let mut admission_unknown = admission(&record, 150, candidate.clone());
    admission_unknown["extra"] = json!(1);
    assert!(CandidateAdmissionEvaluator
        .evaluate(&admission_unknown)
        .is_err());

    let mut record_unknown = admission(&record, 150, candidate.clone());
    record_unknown["baseline"]["hidden"] = json!("x");
    assert!(CandidateAdmissionEvaluator
        .evaluate(&record_unknown)
        .is_err());
}

#[test]
fn admission_requires_the_bound_baseline_identity_and_digest() {
    let record = baseline_record();
    let other = other_record();
    let candidate = candidate(&record);

    // A candidate bound to another baseline identity and digest is refused on both axes.
    let foreign = QualityCandidate::new(
        "candidate:two",
        other.baseline_id.as_str(),
        other.baseline_digest.as_str(),
        other.suite_digest.as_str(),
        CandidateDimension::Prompt,
        base_versions(),
        sha('9'),
        "quality-owner",
    )
    .expect("foreign candidate");
    let findings = admit(&admission(&record, 150, foreign));
    assert_eq!(
        codes(&findings),
        vec![
            "candidate.baseline_identity_mismatch",
            "candidate.baseline_identity_mismatch"
        ]
    );

    // Keeping the identity but not the digest is refused.
    let mut stale_digest = candidate.clone();
    stale_digest.baseline_digest = other.baseline_digest.clone();
    seal(&mut stale_digest);
    let findings = admit(&admission(&record, 150, stale_digest));
    assert_eq!(
        codes(&findings),
        vec!["candidate.baseline_identity_mismatch"]
    );
    assert_eq!(findings[0].expected, json!("bound_baseline_digest"));
    assert_eq!(findings[0].actual, json!("baseline_digest"));

    // A candidate for another suite is refused.
    let mut foreign_suite = candidate.clone();
    foreign_suite.suite_digest = sha('e');
    seal(&mut foreign_suite);
    let findings = admit(&admission(&record, 150, foreign_suite));
    assert_eq!(codes(&findings), vec!["candidate.suite_digest_mismatch"]);
    assert_eq!(findings[0].actual, json!("suite_digest"));

    // A tampered record is refused before the candidate is compared.
    let mut tampered_record = record.clone();
    tampered_record.baseline_id = "baseline:rewritten".to_owned();
    let findings = admit(&admission(&tampered_record, 150, candidate.clone()));
    assert_eq!(codes(&findings), vec!["candidate.baseline_invalid"]);
    assert_eq!(findings[0].actual, json!("baseline_record_digest_mismatch"));

    // The untouched pair is admissible.
    assert!(admit(&admission(&record, 150, candidate)).is_empty());
}

#[test]
fn admission_refuses_a_baseline_outside_its_validity_window() {
    let record = baseline_record();
    let candidate = candidate(&record);

    let before = admit(&admission(&record, 99, candidate.clone()));
    assert_eq!(codes(&before), vec!["candidate.baseline_not_yet_valid"]);
    assert_eq!(before[0].actual, json!("not_yet_valid"));

    let expired = admit(&admission(&record, 200, candidate.clone()));
    assert_eq!(codes(&expired), vec!["candidate.baseline_stale"]);
    assert_eq!(expired[0].actual, json!("expired"));

    // The window is inclusive at its start and exclusive at its end.
    assert!(admit(&admission(&record, 100, candidate.clone())).is_empty());
    assert!(admit(&admission(&record, 199, candidate)).is_empty());
}

#[test]
fn admission_refuses_a_candidate_that_is_not_a_draft() {
    let record = baseline_record();
    let candidate = candidate(&record);

    for status in [
        CandidateStatus::OfflineEvaluated,
        CandidateStatus::Shadowed,
        CandidateStatus::Approved,
        CandidateStatus::Rejected,
        CandidateStatus::RolledBack,
        CandidateStatus::Deprecated,
    ] {
        let mut reclassified = candidate.clone();
        reclassified.status = status;
        seal(&mut reclassified);
        assert!(reclassified.validate().is_ok());
        let findings = admit(&admission(&record, 150, reclassified));
        assert_eq!(codes(&findings), vec!["candidate.status_not_admissible"]);
        assert_eq!(findings[0].expected, json!("draft_candidate"));
        assert_eq!(findings[0].actual, json!(status.key()));
    }

    assert!(admit(&admission(&record, 150, candidate)).is_empty());
}

#[test]
fn invalid_candidates_are_never_admitted() {
    let record = baseline_record();
    let candidate = candidate(&record);

    let mut tampered = candidate.clone();
    tampered.candidate_digest = sha('f');
    let findings = admit(&admission(&record, 150, tampered));
    assert_eq!(codes(&findings), vec!["candidate.binding_invalid"]);
    assert_eq!(findings[0].actual, json!("candidate_digest_mismatch"));

    // An invalid candidate short-circuits every baseline check, even a stale window.
    let mut invalid = candidate.clone();
    invalid.snapshot_digest = sha('f');
    let findings = admit(&admission(&record, 200, invalid));
    assert_eq!(codes(&findings), vec!["candidate.binding_invalid"]);
    assert_eq!(
        findings[0].actual,
        json!("candidate_snapshot_digest_mismatch")
    );

    // No admission verdict is a side effect: repeating it yields the same findings only.
    assert!(admit(&admission(&record, 150, candidate)).is_empty());
}

#[test]
fn a_single_prompt_change_is_admissible_against_its_baseline() {
    let record = baseline_record();
    let candidate = candidate(&record);

    assert_eq!(candidate.schema, CANDIDATE_SCHEMA);
    assert_eq!(candidate.status, CandidateStatus::Draft);
    assert_eq!(candidate.changed_dimension, CandidateDimension::Prompt);
    assert_eq!(candidate.changed_dimension.key(), "prompt");
    assert_eq!(candidate.base_versions.len(), MAX_CANDIDATE_DIMENSIONS);
    assert_eq!(
        candidate.passive_version_refs.len() + 1,
        MAX_CANDIDATE_DIMENSIONS
    );
    assert_eq!(candidate.base_versions.get("prompt"), Some(&sha('2')));
    assert_eq!(candidate.passive_version_refs.get("prompt"), None);
    assert_eq!(
        candidate.snapshot_versions.get("prompt"),
        Some(&candidate.changed_version_ref)
    );
    assert_eq!(
        CandidateDimension::from_key("prompt"),
        Some(CandidateDimension::Prompt)
    );
    assert_eq!(CandidateDimension::from_key("hidden"), None);
    assert_eq!(
        QualityCandidate::snapshot_digest(&candidate.snapshot_versions),
        candidate.snapshot_digest
    );

    assert!(candidate.validate().is_ok());
    assert!(evaluate(&input(candidate.clone())).is_empty());
    assert!(admit(&admission(&record, 150, candidate)).is_empty());
}

#[test]
fn admission_output_is_deterministic_and_registry_wired() {
    let record = baseline_record();
    let mut reclassified = candidate(&record);
    reclassified.status = CandidateStatus::Approved;
    seal(&mut reclassified);
    let value = admission(&record, 200, reclassified);

    let first = admit(&value);
    let second = admit(&value);
    assert_eq!(first, second);
    assert_eq!(
        codes(&first),
        vec![
            "candidate.baseline_stale",
            "candidate.status_not_admissible"
        ]
    );
    assert_eq!(
        canonical_findings(first.clone()).expect("canonical findings"),
        canonical_findings(second).expect("canonical findings")
    );

    let mut registry = EvaluatorRegistry::new();
    registry
        .register(CandidateEvaluator)
        .expect("register candidate evaluator");
    registry
        .register(CandidateAdmissionEvaluator)
        .expect("register candidate admission evaluator");
    assert_eq!(
        registry.evaluator_ids(),
        vec![
            CANDIDATE_ADMISSION_EVALUATOR_ID.to_owned(),
            CANDIDATE_EVALUATOR_ID.to_owned()
        ]
    );

    let mut admission_registry = EvaluatorRegistry::new();
    admission_registry
        .register(CandidateAdmissionEvaluator)
        .expect("register admission evaluator");
    assert_eq!(
        canonical_findings(admission_registry.run(&value).expect("registry run"))
            .expect("registry findings"),
        canonical_findings(first).expect("direct findings")
    );

    let mut candidate_registry = EvaluatorRegistry::new();
    candidate_registry
        .register(CandidateEvaluator)
        .expect("register candidate evaluator");
    let candidate_value = input(candidate(&record));
    assert!(candidate_registry
        .run(&candidate_value)
        .expect("registry run")
        .is_empty());
    assert!(evaluate(&candidate_value).is_empty());
}

#[test]
fn typed_constructors_round_trip() {
    let record = baseline_record();
    assert!(record.validate().is_ok());

    let candidate = candidate(&record);
    let value = serde_json::to_value(&candidate).expect("candidate value");
    assert_eq!(value["schema"], json!(CANDIDATE_SCHEMA));
    let decoded_candidate: QualityCandidate =
        serde_json::from_value(value.clone()).expect("candidate decode");
    assert_eq!(decoded_candidate, candidate);
    assert_eq!(decoded_candidate.digest(), candidate.candidate_digest);

    let candidate_input = CandidateInput::new(candidate.clone());
    assert_eq!(candidate_input.schema, CANDIDATE_INPUT_SCHEMA);
    assert!(candidate_input.validate().is_ok());
    let value = candidate_input.as_value();
    assert_eq!(value["schema"], json!(CANDIDATE_INPUT_SCHEMA));
    assert_eq!(
        serde_json::from_value::<CandidateInput>(value.clone()).expect("candidate input decode"),
        candidate_input
    );

    let admission_input = CandidateAdmissionInput::new(150, record.clone(), candidate.clone());
    assert_eq!(admission_input.schema, CANDIDATE_ADMISSION_INPUT_SCHEMA);
    assert!(admission_input.validate().is_ok());
    let value = admission_input.as_value();
    assert_eq!(value["schema"], json!(CANDIDATE_ADMISSION_INPUT_SCHEMA));
    assert_eq!(
        serde_json::from_value::<CandidateAdmissionInput>(value.clone()).expect("admission decode"),
        admission_input
    );

    for (status, key) in [
        (CandidateStatus::Draft, "draft"),
        (CandidateStatus::OfflineEvaluated, "offline_evaluated"),
        (CandidateStatus::Shadowed, "shadowed"),
        (CandidateStatus::Approved, "approved"),
        (CandidateStatus::Rejected, "rejected"),
        (CandidateStatus::RolledBack, "rolled_back"),
        (CandidateStatus::Deprecated, "deprecated"),
    ] {
        assert_eq!(status.key(), key);
    }
    for (dimension, key) in [
        (CandidateDimension::Model, "model"),
        (CandidateDimension::Prompt, "prompt"),
        (CandidateDimension::ToolCatalog, "tool_catalog"),
        (CandidateDimension::MemoryIndex, "memory_index"),
        (CandidateDimension::Workflow, "workflow"),
        (CandidateDimension::Route, "route"),
    ] {
        assert_eq!(dimension.key(), key);
        assert_eq!(CandidateDimension::from_key(key), Some(dimension));
    }
}
