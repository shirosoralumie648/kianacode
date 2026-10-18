use kiana_quality::{
    canonical_findings, BaselineComparisonInput, BaselineEvaluator, BaselineRecord,
    BaselineRegistry, BaselineRegistryEvaluator, BaselineRegistryInput, DeterministicEvaluator,
    EvaluatorRegistry, Finding, BASELINE_EVALUATOR_ID, BASELINE_INPUT_SCHEMA,
    BASELINE_LOOKUP_EVALUATOR_ID, BASELINE_RECORD_SCHEMA, BASELINE_REGISTRY_INPUT_SCHEMA,
    BASELINE_REGISTRY_SCHEMA, MAX_BASELINE_ENTRIES, MAX_BASELINE_TEXT_BYTES, MAX_BASELINE_TTL_MS,
};
use serde_json::{json, Value};

fn sha(hex: char) -> String {
    format!("sha256:{}", hex.to_string().repeat(64))
}

fn record() -> BaselineRecord {
    BaselineRecord::new(
        "baseline:one",
        sha('a'),
        sha('a'),
        sha('a'),
        sha('a'),
        "quality-owner",
        100,
        200,
        "capture:one",
    )
}

fn refreshed_record(
    predecessor: &BaselineRecord,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    refresh_provenance: &str,
) -> BaselineRecord {
    BaselineRecord::new(
        "baseline:two",
        sha('a'),
        sha('a'),
        sha('a'),
        sha('a'),
        "quality-owner",
        created_at_unix_ms,
        expires_at_unix_ms,
        refresh_provenance,
    )
    .refreshed_from(predecessor.baseline_digest.as_str())
}

fn comparison(record: BaselineRecord, now_unix_ms: u64, digest: &str) -> Value {
    json!({
        "schema": BASELINE_INPUT_SCHEMA,
        "baseline": record,
        "now_unix_ms": now_unix_ms,
        "suite_digest": digest,
        "case_digest": digest,
        "target_digest": digest,
        "evaluator_digest": digest,
    })
}

fn lookup(registry: &BaselineRegistry, baseline_id: &str, now_unix_ms: u64, digest: &str) -> Value {
    json!({
        "schema": BASELINE_REGISTRY_INPUT_SCHEMA,
        "registry": registry,
        "baseline_id": baseline_id,
        "now_unix_ms": now_unix_ms,
        "suite_digest": digest,
        "case_digest": digest,
        "target_digest": digest,
        "evaluator_digest": digest,
    })
}

fn decoded_comparison(value: &Value) -> BaselineComparisonInput {
    serde_json::from_value(value.clone()).expect("baseline comparison envelope")
}

fn decoded_lookup(value: &Value) -> BaselineRegistryInput {
    serde_json::from_value(value.clone()).expect("baseline registry envelope")
}

fn compare(value: &Value) -> Vec<Finding> {
    BaselineEvaluator
        .evaluate(value)
        .expect("baseline comparison evaluation")
}

fn resolve(value: &Value) -> Vec<Finding> {
    BaselineRegistryEvaluator
        .evaluate(value)
        .expect("baseline registry evaluation")
}

fn codes(findings: &[Finding]) -> Vec<&str> {
    findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

/// Apply one mutation to a fresh record and validate the result.
fn mutate(
    record: &BaselineRecord,
    apply: impl FnOnce(&mut BaselineRecord),
) -> Result<(), &'static str> {
    let mut candidate = record.clone();
    apply(&mut candidate);
    candidate.validate()
}

#[test]
fn compatible_unexpired_baseline_can_compare() {
    let baseline = record();
    assert_eq!(baseline.schema, BASELINE_RECORD_SCHEMA);
    assert!(baseline.validate().is_ok());
    assert!(baseline.baseline_digest.starts_with("sha256:"));
    assert!(baseline.refreshed_from_baseline_digest.is_empty());

    let digest = sha('a');
    let findings = compare(&comparison(baseline.clone(), 150, &digest));
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");

    let typed = BaselineComparisonInput::new(
        baseline,
        150,
        digest.clone(),
        digest.clone(),
        digest.clone(),
        digest,
    );
    assert!(typed.validate().is_ok());
    assert!(compare(&typed.as_value()).is_empty());
}

#[test]
fn stale_or_incompatible_baseline_cannot_compare() {
    let digest = sha('a');
    let other = sha('b');

    let stale = compare(&comparison(record(), 200, &digest));
    assert_eq!(codes(&stale), vec!["baseline.stale"]);
    assert_eq!(stale[0].expected, json!("unexpired_baseline"));
    assert_eq!(stale[0].actual, json!("expired"));
    assert_eq!(stale[0].evidence_ref, "baseline:input");

    let last_valid_moment = compare(&comparison(record(), 199, &digest));
    assert!(last_valid_moment.is_empty());

    for axis in [
        "suite_digest",
        "case_digest",
        "target_digest",
        "evaluator_digest",
    ] {
        let mut input = comparison(record(), 150, &digest);
        input[axis] = json!(other.clone());
        let findings = compare(&input);
        assert_eq!(codes(&findings), vec!["baseline.incompatible"]);
        assert_eq!(findings[0].expected, json!(axis));
        assert_eq!(findings[0].actual, json!("digest_mismatch"));
    }

    // Drift on every axis is reported once per axis and never counts as comparable.
    let drifted = compare(&comparison(record(), 150, &other));
    assert_eq!(drifted.len(), 4);

    let mut stale_and_drifted = comparison(record(), 150, &other);
    stale_and_drifted["now_unix_ms"] = json!(500);
    assert_eq!(
        codes(&compare(&stale_and_drifted)),
        vec![
            "baseline.stale",
            "baseline.incompatible",
            "baseline.incompatible",
            "baseline.incompatible",
            "baseline.incompatible",
        ]
    );
}

#[test]
fn tampered_baseline_records_fail_closed() {
    let mut rewritten = record();
    rewritten.baseline_digest = sha('f');
    assert_eq!(rewritten.validate(), Err("baseline_record_digest_mismatch"));
    let findings = compare(&comparison(rewritten, 150, &sha('a')));
    assert_eq!(codes(&findings), vec!["baseline.record_invalid"]);
    assert_eq!(findings[0].expected, json!("validated_baseline_record"));
    assert_eq!(findings[0].actual, json!("baseline_record_digest_mismatch"));
    assert_eq!(findings[0].evidence_ref, "baseline:input");

    // Editing a bound field after the digest was computed is the same refusal.
    assert_eq!(
        mutate(&record(), |candidate| candidate.owner_id =
            "quality-owner-two".to_owned()),
        Err("baseline_record_digest_mismatch")
    );

    assert_eq!(
        mutate(&record(), |candidate| candidate.schema =
            "kiana.quality-baseline-record.v2".to_owned()),
        Err("baseline_record_schema_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.baseline_id = String::new()),
        Err("baseline_record_identity_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.owner_id =
            "quality owner".to_owned()),
        Err("baseline_record_identity_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.refresh_provenance =
            "\n".to_owned()),
        Err("baseline_record_identity_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.baseline_id =
            "b".repeat(MAX_BASELINE_TEXT_BYTES + 1)),
        Err("baseline_record_identity_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.created_at_unix_ms = 0),
        Err("baseline_record_window_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.expires_at_unix_ms = 100),
        Err("baseline_record_window_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.expires_at_unix_ms = 50),
        Err("baseline_record_window_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.expires_at_unix_ms =
            100 + MAX_BASELINE_TTL_MS + 1),
        Err("baseline_record_window_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.suite_digest =
            "sha256:AAAA".to_owned()),
        Err("baseline_record_digest_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.case_digest =
            format!("sha256:{}", "a".repeat(63))),
        Err("baseline_record_digest_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate.target_digest =
            sha('a').to_uppercase()),
        Err("baseline_record_digest_invalid")
    );
    let own_digest = record().baseline_digest;
    assert_eq!(
        mutate(&record(), |candidate| candidate
            .refreshed_from_baseline_digest =
            own_digest.clone()),
        Err("baseline_record_digest_invalid")
    );
    assert_eq!(
        mutate(&record(), |candidate| candidate
            .refreshed_from_baseline_digest =
            "sha256:zz".to_owned()),
        Err("baseline_record_digest_invalid")
    );
}

#[test]
fn expiry_window_is_bounded_and_exclusive() {
    let digest = sha('a');

    assert!(compare(&comparison(record(), 100, &digest)).is_empty());
    assert_eq!(
        codes(&compare(&comparison(record(), 99, &digest))),
        vec!["baseline.not_yet_valid"]
    );
    assert!(compare(&comparison(record(), 199, &digest)).is_empty());
    assert_eq!(
        codes(&compare(&comparison(record(), 200, &digest))),
        vec!["baseline.stale"]
    );

    // The maximum window is accepted; one millisecond more is not.
    assert!(mutate(&record(), |candidate| candidate.expires_at_unix_ms =
        100 + MAX_BASELINE_TTL_MS)
    .is_ok());

    // An unset comparison time is an envelope refusal, not a comparison result.
    assert!(BaselineEvaluator
        .evaluate(&comparison(record(), 0, &digest))
        .is_err());
}

#[test]
fn comparison_envelope_fails_closed() {
    let digest = sha('a');

    let mut drifted_schema = comparison(record(), 150, &digest);
    drifted_schema["schema"] = json!("kiana.quality-baseline-input.v2");
    assert_eq!(
        decoded_comparison(&drifted_schema).validate(),
        Err("baseline_input_schema_invalid")
    );
    assert!(BaselineEvaluator.evaluate(&drifted_schema).is_err());

    let mut unset_time = comparison(record(), 150, &digest);
    unset_time["now_unix_ms"] = json!(0);
    assert_eq!(
        decoded_comparison(&unset_time).validate(),
        Err("baseline_input_time_invalid")
    );

    let mut bad_digest = comparison(record(), 150, &digest);
    bad_digest["suite_digest"] = json!("not-a-digest");
    assert_eq!(
        decoded_comparison(&bad_digest).validate(),
        Err("baseline_input_digest_invalid")
    );
    assert!(BaselineEvaluator.evaluate(&bad_digest).is_err());

    let mut uppercase_hex = comparison(record(), 150, &digest);
    uppercase_hex["case_digest"] = json!(format!("sha256:{}", "A".repeat(64)));
    assert!(BaselineEvaluator.evaluate(&uppercase_hex).is_err());

    let mut wrong_type = comparison(record(), 150, &digest);
    wrong_type["now_unix_ms"] = json!("not-a-number");
    assert!(BaselineEvaluator.evaluate(&wrong_type).is_err());

    let mut unknown_field = comparison(record(), 150, &digest);
    unknown_field["unexpected"] = json!(true);
    assert!(BaselineEvaluator.evaluate(&unknown_field).is_err());

    assert!(BaselineComparisonInput::new(
        record(),
        0,
        digest.clone(),
        digest.clone(),
        digest.clone(),
        digest,
    )
    .validate()
    .is_err());
}

#[test]
fn registry_requires_digest_binding_and_bounded_entries() {
    let registry = BaselineRegistry::new(vec![record()]);
    assert_eq!(registry.schema, BASELINE_REGISTRY_SCHEMA);
    assert!(registry.validate().is_ok());
    assert!(registry.registry_digest.starts_with("sha256:"));
    assert!(registry.lookup("baseline:one").is_some());
    assert!(registry.lookup("baseline:missing").is_none());

    assert_eq!(
        BaselineRegistry::new(Vec::new()).validate(),
        Err("baseline_registry_empty")
    );

    let mut tampered = registry.clone();
    tampered.registry_digest = sha('f');
    assert_eq!(
        tampered.validate(),
        Err("baseline_registry_digest_mismatch")
    );

    let mut rewritten = registry.clone();
    rewritten.entries[0].owner_id = "quality-owner-two".to_owned();
    assert_eq!(
        rewritten.validate(),
        Err("baseline_registry_digest_mismatch")
    );

    let mut drifted = registry.clone();
    drifted.schema = "kiana.quality-baseline-registry.v2".to_owned();
    assert_eq!(drifted.validate(), Err("baseline_registry_schema_invalid"));

    let mut undigested = registry.clone();
    undigested.registry_digest = "not-a-digest".to_owned();
    assert_eq!(
        undigested.validate(),
        Err("baseline_registry_digest_invalid")
    );

    let many: Vec<BaselineRecord> = (0..=MAX_BASELINE_ENTRIES)
        .map(|index| {
            BaselineRecord::new(
                format!("baseline:{index}"),
                format!("sha256:{index:064x}"),
                sha('a'),
                sha('a'),
                sha('a'),
                "quality-owner",
                100,
                200,
                format!("capture:{index}"),
            )
        })
        .collect();
    assert_eq!(many.len(), MAX_BASELINE_ENTRIES + 1);
    assert_eq!(
        BaselineRegistry::new(many).validate(),
        Err("baseline_registry_size_exceeded")
    );
}

#[test]
fn registry_rejects_duplicate_or_tampered_entries() {
    let duplicate = BaselineRegistry::new(vec![record(), record()]);
    assert_eq!(duplicate.validate(), Err("baseline_registry_duplicate"));

    let mut tampered_entry = record();
    tampered_entry.suite_digest = sha('b');
    assert_eq!(
        BaselineRegistry::new(vec![tampered_entry]).validate(),
        Err("baseline_record_digest_mismatch")
    );
}

#[test]
fn refreshed_baselines_must_link_the_predecessor() {
    let first = record();
    let second = refreshed_record(&first, 200, 400, "capture:two");
    let registry = BaselineRegistry::new(vec![first.clone(), second]);
    assert!(registry.validate().is_ok());
    assert_eq!(registry.entries.len(), 2);

    assert!(resolve(&lookup(&registry, "baseline:one", 150, &sha('a'))).is_empty());
    assert!(resolve(&lookup(&registry, "baseline:two", 250, &sha('a'))).is_empty());
    assert_eq!(
        codes(&resolve(&lookup(&registry, "baseline:one", 250, &sha('a')))),
        vec!["baseline.stale"]
    );

    let unlinked = BaselineRecord::new(
        "baseline:two",
        sha('a'),
        sha('a'),
        sha('a'),
        sha('a'),
        "quality-owner",
        200,
        400,
        "capture:two",
    );
    assert_eq!(
        BaselineRegistry::new(vec![first.clone(), unlinked]).validate(),
        Err("baseline_registry_provenance_mismatch")
    );

    let wrong_predecessor = BaselineRecord::new(
        "baseline:two",
        sha('a'),
        sha('a'),
        sha('a'),
        sha('a'),
        "quality-owner",
        200,
        400,
        "capture:two",
    )
    .refreshed_from(sha('f'));
    assert_eq!(
        BaselineRegistry::new(vec![first.clone(), wrong_predecessor]).validate(),
        Err("baseline_registry_provenance_mismatch")
    );

    let reused_provenance = refreshed_record(&first, 200, 400, "capture:one");
    assert_eq!(
        BaselineRegistry::new(vec![first.clone(), reused_provenance]).validate(),
        Err("baseline_registry_provenance_reused")
    );

    let overlapping = refreshed_record(&first, 150, 400, "capture:two");
    assert_eq!(
        BaselineRegistry::new(vec![first, overlapping]).validate(),
        Err("baseline_registry_overlap")
    );
}

#[test]
fn registry_lookup_resolves_the_named_baseline() {
    let registry = BaselineRegistry::new(vec![record()]);

    let typed = BaselineRegistryInput::new(
        registry.clone(),
        "baseline:one",
        150,
        sha('a'),
        sha('a'),
        sha('a'),
        sha('a'),
    );
    assert!(typed.validate().is_ok());
    assert!(resolve(&typed.as_value()).is_empty());

    let missing = resolve(&lookup(&registry, "baseline:missing", 150, &sha('a')));
    assert_eq!(codes(&missing), vec!["baseline.record_missing"]);
    assert_eq!(missing[0].expected, json!("registered_baseline_record"));
    assert_eq!(missing[0].actual, json!("missing"));
    assert_eq!(missing[0].evidence_ref, "baseline:registry");

    let mut tampered_record = record();
    tampered_record.owner_id = "quality-owner-two".to_owned();
    let tampered_registry = BaselineRegistry::new(vec![tampered_record]);
    let invalid = resolve(&lookup(&tampered_registry, "baseline:one", 150, &sha('a')));
    assert_eq!(codes(&invalid), vec!["baseline.registry_invalid"]);
    assert_eq!(invalid[0].actual, json!("baseline_record_digest_mismatch"));

    let stale = resolve(&lookup(&registry, "baseline:one", 200, &sha('a')));
    assert_eq!(codes(&stale), vec!["baseline.stale"]);

    let mut drifted = lookup(&registry, "baseline:one", 150, &sha('a'));
    drifted["target_digest"] = json!(sha('b'));
    let incompatible = resolve(&drifted);
    assert_eq!(codes(&incompatible), vec!["baseline.incompatible"]);
    assert_eq!(incompatible[0].expected, json!("target_digest"));
}

#[test]
fn lookup_envelope_fails_closed() {
    let registry = BaselineRegistry::new(vec![record()]);
    let digest = sha('a');

    let mut drifted_schema = lookup(&registry, "baseline:one", 150, &digest);
    drifted_schema["schema"] = json!("kiana.quality-baseline-registry-input.v2");
    assert_eq!(
        decoded_lookup(&drifted_schema).validate(),
        Err("baseline_registry_input_schema_invalid")
    );
    assert!(BaselineRegistryEvaluator.evaluate(&drifted_schema).is_err());

    for identity in ["", "   "] {
        let mut blank_identity = lookup(&registry, "baseline:one", 150, &digest);
        blank_identity["baseline_id"] = json!(identity);
        assert_eq!(
            decoded_lookup(&blank_identity).validate(),
            Err("baseline_registry_input_identity_invalid")
        );
    }

    let mut unset_time = lookup(&registry, "baseline:one", 150, &digest);
    unset_time["now_unix_ms"] = json!(0);
    assert_eq!(
        decoded_lookup(&unset_time).validate(),
        Err("baseline_input_time_invalid")
    );
    assert!(BaselineRegistryEvaluator.evaluate(&unset_time).is_err());

    let mut bad_digest = lookup(&registry, "baseline:one", 150, &digest);
    bad_digest["evaluator_digest"] = json!(sha('a').to_uppercase());
    assert!(BaselineRegistryEvaluator.evaluate(&bad_digest).is_err());

    let mut unknown_field = lookup(&registry, "baseline:one", 150, &digest);
    unknown_field["extra"] = json!(1);
    assert!(BaselineRegistryEvaluator.evaluate(&unknown_field).is_err());
}

#[test]
fn baseline_output_is_deterministic_and_registry_wired() {
    let registry = BaselineRegistry::new(vec![record()]);
    let input = lookup(&registry, "baseline:one", 200, &sha('b'));
    let first = resolve(&input);
    let second = resolve(&input);
    assert_eq!(first, second);
    assert_eq!(
        codes(&first),
        vec![
            "baseline.stale",
            "baseline.incompatible",
            "baseline.incompatible",
            "baseline.incompatible",
            "baseline.incompatible",
        ]
    );
    assert_eq!(
        canonical_findings(first.clone()).expect("canonical findings"),
        canonical_findings(second).expect("canonical findings")
    );

    let mut lookup_registry = EvaluatorRegistry::new();
    lookup_registry
        .register(BaselineRegistryEvaluator)
        .expect("register baseline lookup evaluator");
    assert_eq!(
        lookup_registry.evaluator_ids(),
        vec![BASELINE_LOOKUP_EVALUATOR_ID.to_owned()]
    );
    assert_eq!(
        canonical_findings(lookup_registry.run(&input).expect("registry run"))
            .expect("registry findings"),
        canonical_findings(first).expect("direct findings")
    );

    let mut comparison_registry = EvaluatorRegistry::new();
    comparison_registry
        .register(BaselineEvaluator)
        .expect("register baseline comparison evaluator");
    assert_eq!(
        comparison_registry.evaluator_ids(),
        vec![BASELINE_EVALUATOR_ID.to_owned()]
    );
    let direct = comparison(record(), 150, &sha('a'));
    assert!(compare(&direct).is_empty());
    assert_eq!(
        canonical_findings(comparison_registry.run(&direct).expect("registry run"))
            .expect("registry findings"),
        canonical_findings(compare(&direct)).expect("direct findings")
    );
}

#[test]
fn typed_constructors_round_trip() {
    let baseline = record();
    assert!(baseline.validate().is_ok());
    let value = serde_json::to_value(&baseline).expect("baseline value");
    let decoded: BaselineRecord = serde_json::from_value(value.clone()).expect("baseline decode");
    assert_eq!(decoded, baseline);
    assert_eq!(decoded.digest(), baseline.baseline_digest);

    let registry = BaselineRegistry::new(vec![baseline.clone()]);
    let value = serde_json::to_value(&registry).expect("registry value");
    let decoded: BaselineRegistry = serde_json::from_value(value.clone()).expect("registry decode");
    assert_eq!(decoded, registry);
    assert_eq!(value["schema"], json!(BASELINE_REGISTRY_SCHEMA));

    let digest = sha('a');
    let comparison_input = BaselineComparisonInput::new(
        baseline,
        150,
        digest.clone(),
        digest.clone(),
        digest.clone(),
        digest.clone(),
    );
    let value = comparison_input.as_value();
    assert_eq!(value["schema"], json!(BASELINE_INPUT_SCHEMA));
    assert_eq!(
        serde_json::from_value::<BaselineComparisonInput>(value.clone())
            .expect("comparison decode"),
        comparison_input
    );

    let lookup_input = BaselineRegistryInput::new(
        registry,
        "baseline:one",
        150,
        digest.clone(),
        digest.clone(),
        digest.clone(),
        digest,
    );
    let value = lookup_input.as_value();
    assert_eq!(value["schema"], json!(BASELINE_REGISTRY_INPUT_SCHEMA));
    assert_eq!(
        serde_json::from_value::<BaselineRegistryInput>(value.clone()).expect("lookup decode"),
        lookup_input
    );
}
