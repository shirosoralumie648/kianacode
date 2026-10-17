use kiana_quality::{
    canonical_findings, classify_flake, digest_findings, evaluate_flake,
    flake_attempt_evidence_digest, AttemptOutcome, DeterministicEvaluator, EvaluatorRegistry,
    Finding, FlakeAttempt, FlakeClassification, FlakeClassifierEvaluator, FlakeFinalStatus,
    FlakeInput, FlakeQuarantine, InfraFailureClass, FLAKE_ATTEMPT_SCHEMA, FLAKE_EVALUATOR_ID,
    FLAKE_INPUT_SCHEMA, FLAKE_QUARANTINE_SCHEMA, MAX_FLAKE_TEXT_BYTES, MAX_QUARANTINE_TTL_MS,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const CASE: &str = "case:retry-once";
const NOW: u64 = 1_700_000_000_000;
const SEED: u64 = 42;
const HALF_TTL: u64 = MAX_QUARANTINE_TTL_MS / 2;
const SECRET_CASE: &str = "token=SHOULD_NOT_LEAK_1234567890";

/// Deterministic fake sha256 attempt-evidence digest.
fn sha(hex: char) -> String {
    format!("sha256:{}", hex.to_string().repeat(64))
}

fn attempt(number: u8, outcome: AttemptOutcome, seed: u64, digest: &str) -> FlakeAttempt {
    FlakeAttempt::new(number, CASE, seed, outcome, digest)
}

/// First attempt failed, one fixed-seed retry passed: the documented flake shape.
fn flaky_attempts() -> Vec<FlakeAttempt> {
    vec![
        attempt(1, AttemptOutcome::Fail, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
    ]
}

/// The same failure twice: a real failure, not a flake.
fn repeated_failure_attempts() -> Vec<FlakeAttempt> {
    vec![
        attempt(1, AttemptOutcome::Fail, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Fail, SEED, &sha('b')),
    ]
}

fn quarantine(attempts: &[FlakeAttempt], expires_at_unix_ms: u64) -> FlakeQuarantine {
    FlakeQuarantine::new(
        CASE,
        "first_failure_then_pass",
        "quality-owner",
        expires_at_unix_ms,
        flake_attempt_evidence_digest(attempts),
    )
}

/// A quarantine record that is still inside its bounded window at `NOW`.
fn live_quarantine(attempts: &[FlakeAttempt]) -> FlakeQuarantine {
    quarantine(attempts, NOW + HALF_TTL)
}

fn input(
    attempts: &[FlakeAttempt],
    final_status: FlakeFinalStatus,
    quarantine: Option<FlakeQuarantine>,
) -> Value {
    let mut envelope = FlakeInput::new(CASE, NOW, attempts.to_vec(), final_status);
    if let Some(record) = quarantine {
        envelope = envelope.with_quarantine(record);
    }
    envelope.as_value()
}

fn evaluate(value: &Value) -> Vec<Finding> {
    FlakeClassifierEvaluator
        .evaluate(value)
        .expect("flake fixture decodes")
}

fn evaluate_error(value: &Value) -> String {
    FlakeClassifierEvaluator
        .evaluate(value)
        .expect_err("fixture must be refused")
        .to_string()
}

fn codes(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect()
}

fn set(codes: &[&str]) -> BTreeSet<String> {
    codes.iter().map(|code| (*code).to_owned()).collect()
}

#[test]
fn flake_is_never_counted_as_pass() {
    // Card acceptance: a first failure followed by a fixed-seed pass is flaky and can never be
    // reported as a pass.
    let flaky = flaky_attempts();
    let as_pass = evaluate(&input(&flaky, FlakeFinalStatus::Pass, None));
    assert_eq!(
        codes(&as_pass),
        set(&["flake.flaky_counted_as_pass", "flake.quarantine_missing",])
    );
    let counted = as_pass
        .iter()
        .find(|finding| finding.code == "flake.flaky_counted_as_pass")
        .expect("flaky counted as pass finding");
    assert_eq!(counted.evidence_ref, "flake:status");
    assert_eq!(counted.actual["classification"], json!("flake"));

    // The flake may not be laundered into the other case-result status either.
    assert_eq!(
        codes(&evaluate(&input(&flaky, FlakeFinalStatus::Fail, None))),
        set(&["flake.flaky_counted_as_result", "flake.quarantine_missing",])
    );

    // Only `needs_review` and `quarantined` are accepted, and both require the record.
    for status in [FlakeFinalStatus::NeedsReview, FlakeFinalStatus::Quarantined] {
        assert!(
            evaluate(&input(&flaky, status, Some(live_quarantine(&flaky)))).is_empty(),
            "flaky case must stay visible under {status:?}"
        );
    }

    let decoded: FlakeInput =
        serde_json::from_value(input(&flaky, FlakeFinalStatus::Pass, None)).expect("decode");
    assert_eq!(classify_flake(&decoded), FlakeClassification::Flake);
}

#[test]
fn stable_failures_stay_failures_and_never_become_a_flake() {
    let stable = repeated_failure_attempts();
    assert!(evaluate(&input(&stable, FlakeFinalStatus::Fail, None)).is_empty());

    // The same failure twice is a real failure: a pass claim is a status mismatch, not a flake.
    assert_eq!(
        codes(&evaluate(&input(&stable, FlakeFinalStatus::Pass, None))),
        set(&["flake.status_attempt_mismatch"])
    );

    // Quarantine is reserved for unstable outcomes and cannot be attached to a real failure.
    assert_eq!(
        codes(&evaluate(&input(
            &stable,
            FlakeFinalStatus::Quarantined,
            Some(live_quarantine(&stable)),
        ))),
        set(&["flake.status_attempt_mismatch"])
    );
    assert_eq!(
        codes(&evaluate(&input(
            &stable,
            FlakeFinalStatus::Fail,
            Some(live_quarantine(&stable)),
        ))),
        set(&["flake.quarantine_unexpected"])
    );

    // A single failed attempt is also a real failure; a pass claim is refused.
    let single_failure = vec![attempt(1, AttemptOutcome::Fail, SEED, &sha('a'))];
    assert!(evaluate(&input(&single_failure, FlakeFinalStatus::Fail, None)).is_empty());
    assert_eq!(
        codes(&evaluate(&input(
            &single_failure,
            FlakeFinalStatus::Pass,
            None
        ))),
        set(&["flake.status_attempt_mismatch"])
    );
}

#[test]
fn retry_is_bounded_to_one_retry_with_a_fixed_seed() {
    // The retry must reuse the first attempt's fixed seed.
    let mut reseeded = flaky_attempts();
    reseeded[1].seed = SEED + 1;
    assert_eq!(
        codes(&evaluate(&input(
            &reseeded,
            FlakeFinalStatus::Quarantined,
            Some(live_quarantine(&reseeded)),
        ))),
        set(&["flake.seed_mismatch"])
    );

    // More than one retry is a structural refusal.
    let three = vec![
        attempt(1, AttemptOutcome::Fail, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
        attempt(3, AttemptOutcome::Pass, SEED, &sha('c')),
    ];
    assert!(
        evaluate_error(&input(&three, FlakeFinalStatus::Quarantined, None))
            .contains("flake_attempt_limit_exceeded")
    );

    // Two attempts without the retry-once declaration are refused.
    let mut undeclared = input(
        &flaky_attempts(),
        FlakeFinalStatus::Quarantined,
        Some(live_quarantine(&flaky_attempts())),
    );
    undeclared["retry_once"] = json!(false);
    assert_eq!(
        codes(&evaluate(&undeclared)),
        set(&["flake.retry_policy_invalid"])
    );
}

#[test]
fn retry_after_pass_is_refused() {
    let retried: Vec<FlakeAttempt> = vec![
        attempt(1, AttemptOutcome::Pass, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Fail, SEED, &sha('b')),
    ];

    // A retry after a pass is invalid; the first attempt's pass is still the case result.
    assert_eq!(
        codes(&evaluate(&input(&retried, FlakeFinalStatus::Pass, None))),
        set(&["flake.retry_after_pass"])
    );

    let repeated_pass: Vec<FlakeAttempt> = vec![
        attempt(1, AttemptOutcome::Pass, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
    ];
    assert_eq!(
        codes(&evaluate(&input(
            &repeated_pass,
            FlakeFinalStatus::Pass,
            None,
        ))),
        set(&["flake.retry_after_pass"])
    );

    // Presenting the later failure as the case result contradicts the first pass.
    assert_eq!(
        codes(&evaluate(&input(&retried, FlakeFinalStatus::Fail, None))),
        set(&["flake.retry_after_pass", "flake.status_attempt_mismatch",])
    );
    assert_eq!(
        codes(&evaluate(&input(
            &retried,
            FlakeFinalStatus::Fail,
            Some(live_quarantine(&retried)),
        ))),
        set(&[
            "flake.retry_after_pass",
            "flake.status_attempt_mismatch",
            "flake.quarantine_unexpected",
        ])
    );
}

#[test]
fn infra_failures_require_a_known_failure_class() {
    let unclassified = vec![attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))];
    let findings = evaluate(&input(&unclassified, FlakeFinalStatus::Infra, None));
    assert_eq!(codes(&findings), set(&["flake.infra_unclassified"]));
    let finding = findings
        .iter()
        .find(|finding| finding.code == "flake.infra_unclassified")
        .expect("infra binding finding");
    assert_eq!(finding.actual["infra_class"], Value::Null);
    assert_eq!(finding.evidence_ref, "flake:attempt:0");

    let unknown = vec![attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
        .with_infra_class(InfraFailureClass::Unknown)];
    assert_eq!(
        codes(&evaluate(&input(&unknown, FlakeFinalStatus::Infra, None))),
        set(&["flake.infra_unclassified"])
    );

    for class in [
        InfraFailureClass::Runner,
        InfraFailureClass::Fixture,
        InfraFailureClass::Network,
        InfraFailureClass::Resource,
    ] {
        let classified =
            vec![attempt(1, AttemptOutcome::Infra, SEED, &sha('a')).with_infra_class(class)];
        assert!(
            evaluate(&input(&classified, FlakeFinalStatus::Infra, None)).is_empty(),
            "known infra class {class:?} must be accepted"
        );
    }

    // A case result must not smuggle an infra class into its evidence.
    let pass_with_class = vec![attempt(1, AttemptOutcome::Pass, SEED, &sha('a'))
        .with_infra_class(InfraFailureClass::Runner)];
    assert_eq!(
        codes(&evaluate(&input(
            &pass_with_class,
            FlakeFinalStatus::Pass,
            None,
        ))),
        set(&["flake.infra_class_unexpected"])
    );
    let fail_with_class = vec![attempt(1, AttemptOutcome::Fail, SEED, &sha('a'))
        .with_infra_class(InfraFailureClass::Network)];
    assert_eq!(
        codes(&evaluate(&input(
            &fail_with_class,
            FlakeFinalStatus::Fail,
            None,
        ))),
        set(&["flake.infra_class_unexpected"])
    );
}

#[test]
fn infra_attempts_are_never_case_results() {
    let infra = vec![attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
        .with_infra_class(InfraFailureClass::Runner)];

    for status in [FlakeFinalStatus::Pass, FlakeFinalStatus::Fail] {
        assert_eq!(
            codes(&evaluate(&input(&infra, status, None))),
            set(&["flake.infra_counted_as_result"]),
            "infra must not be reported as {status:?}"
        );
    }

    // An infra attempt is not an out-of-scope decision either.
    assert_eq!(
        codes(&evaluate(&input(
            &infra,
            FlakeFinalStatus::OutOfScope,
            None,
        ))),
        set(&["flake.status_attempt_mismatch"])
    );

    // Infra wins over the flake shape: a retry that could not run produced no case result.
    let infra_then_pass = vec![
        attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
            .with_infra_class(InfraFailureClass::Network),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
    ];
    assert_eq!(
        codes(&evaluate(&input(
            &infra_then_pass,
            FlakeFinalStatus::Pass,
            None,
        ))),
        set(&["flake.infra_counted_as_result"])
    );

    // `infra` and `needs_review` are the honest statuses for an infra case.
    assert!(evaluate(&input(&infra, FlakeFinalStatus::Infra, None)).is_empty());
    let infra_then_fail = vec![
        attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
            .with_infra_class(InfraFailureClass::Network),
        attempt(2, AttemptOutcome::Fail, SEED, &sha('b')),
    ];
    assert!(evaluate(&input(
        &infra_then_fail,
        FlakeFinalStatus::NeedsReview,
        None,
    ))
    .is_empty());
    assert!(evaluate(&input(
        &infra,
        FlakeFinalStatus::Quarantined,
        Some(live_quarantine(&infra)),
    ))
    .is_empty());
}

#[test]
fn out_of_scope_attempts_are_never_counted_as_results() {
    let out_of_scope = vec![attempt(1, AttemptOutcome::OutOfScope, SEED, &sha('a'))];

    assert!(evaluate(&input(&out_of_scope, FlakeFinalStatus::OutOfScope, None)).is_empty());
    assert!(evaluate(&input(&out_of_scope, FlakeFinalStatus::NeedsReview, None)).is_empty());

    for status in [
        FlakeFinalStatus::Pass,
        FlakeFinalStatus::Fail,
        FlakeFinalStatus::Infra,
    ] {
        assert_eq!(
            codes(&evaluate(&input(&out_of_scope, status, None))),
            set(&["flake.out_of_scope_counted_as_result"]),
            "out-of-scope must not be reported as {status:?}"
        );
    }

    // Quarantine is not the status for an out-of-scope case.
    assert_eq!(
        codes(&evaluate(&input(
            &out_of_scope,
            FlakeFinalStatus::Quarantined,
            Some(live_quarantine(&out_of_scope)),
        ))),
        set(&["flake.status_attempt_mismatch"])
    );

    // An out-of-scope attempt is not a flake even when a second attempt passes.
    let scope_then_pass = vec![
        attempt(1, AttemptOutcome::OutOfScope, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
    ];
    assert_eq!(
        codes(&evaluate(&input(
            &scope_then_pass,
            FlakeFinalStatus::Pass,
            None,
        ))),
        set(&["flake.out_of_scope_counted_as_result"])
    );
}

#[test]
fn quarantine_records_must_be_bounded() {
    let flaky = flaky_attempts();

    let mut drifted_schema = live_quarantine(&flaky);
    drifted_schema.schema = "kiana.quality-flake-quarantine.v2".to_owned();
    assert_eq!(
        codes(&evaluate(&input(
            &flaky,
            FlakeFinalStatus::Quarantined,
            Some(drifted_schema),
        ))),
        set(&["flake.quarantine_schema_invalid"])
    );

    let mut unowned = live_quarantine(&flaky);
    unowned.owner = String::new();
    let mut spaced_reason = live_quarantine(&flaky);
    spaced_reason.reason = "first failure then pass".to_owned();
    let mut oversized_reason = live_quarantine(&flaky);
    oversized_reason.reason = "r".repeat(MAX_FLAKE_TEXT_BYTES + 1);
    for record in [unowned, spaced_reason, oversized_reason] {
        assert_eq!(
            codes(&evaluate(&input(
                &flaky,
                FlakeFinalStatus::Quarantined,
                Some(record),
            ))),
            set(&["flake.quarantine_invalid"])
        );
    }

    // The documented bound is inclusive.
    let mut bounded_owner = live_quarantine(&flaky);
    bounded_owner.owner = "o".repeat(MAX_FLAKE_TEXT_BYTES);
    assert!(evaluate(&input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(bounded_owner),
    ))
    .is_empty());

    // A secret-shaped owner is refused and never rendered raw.
    let mut secret_owner = live_quarantine(&flaky);
    secret_owner.owner = SECRET_CASE.to_owned();
    let findings = evaluate(&input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(secret_owner),
    ));
    assert_eq!(codes(&findings), set(&["flake.quarantine_invalid"]));
    let serialized = serde_json::to_string(&findings).expect("serialize findings");
    assert!(
        !serialized.contains("SHOULD_NOT_LEAK_1234567890"),
        "raw secret escaped the quarantine finding"
    );

    // A malformed digest is both invalid and not the digest of this evidence.
    let mut forged_digest = live_quarantine(&flaky);
    forged_digest.evidence_digest = "sha256:nope".to_owned();
    assert_eq!(
        codes(&evaluate(&input(
            &flaky,
            FlakeFinalStatus::Quarantined,
            Some(forged_digest),
        ))),
        set(&[
            "flake.quarantine_invalid",
            "flake.quarantine_evidence_mismatch",
        ])
    );
}

#[test]
fn quarantine_must_bind_the_case_and_the_attempt_evidence() {
    let flaky = flaky_attempts();

    let mut wrong_case = live_quarantine(&flaky);
    wrong_case.case_id = "case:other".to_owned();
    assert_eq!(
        codes(&evaluate(&input(
            &flaky,
            FlakeFinalStatus::Quarantined,
            Some(wrong_case),
        ))),
        set(&["flake.quarantine_case_mismatch"])
    );

    // A record copied from another attempt sequence does not match this evidence.
    let mut copied = live_quarantine(&flaky);
    copied.evidence_digest = flake_attempt_evidence_digest(&repeated_failure_attempts());
    assert_eq!(
        codes(&evaluate(&input(
            &flaky,
            FlakeFinalStatus::Quarantined,
            Some(copied),
        ))),
        set(&["flake.quarantine_evidence_mismatch"])
    );

    // Order is part of the binding: swapped attempt numbers do not match.
    let swapped: Vec<FlakeAttempt> = vec![
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
        attempt(1, AttemptOutcome::Fail, SEED, &sha('a')),
    ];
    let mut reordered = live_quarantine(&flaky);
    reordered.evidence_digest = flake_attempt_evidence_digest(&swapped);
    assert_eq!(
        codes(&evaluate(&input(
            &flaky,
            FlakeFinalStatus::Quarantined,
            Some(reordered),
        ))),
        set(&["flake.quarantine_evidence_mismatch"])
    );

    assert!(evaluate(&input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(live_quarantine(&flaky)),
    ))
    .is_empty());
}

#[test]
fn quarantine_must_expire_inside_a_bounded_window() {
    let flaky = flaky_attempts();

    for expired_at in [NOW, NOW - 1, 0] {
        assert_eq!(
            codes(&evaluate(&input(
                &flaky,
                FlakeFinalStatus::Quarantined,
                Some(quarantine(&flaky, expired_at)),
            ))),
            set(&["flake.quarantine_expired"]),
            "quarantine must be live at evaluation time: {expired_at}"
        );
    }

    // The documented lifetime bound is inclusive on both ends.
    assert!(evaluate(&input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(quarantine(&flaky, NOW + MAX_QUARANTINE_TTL_MS)),
    ))
    .is_empty());
    assert_eq!(
        codes(&evaluate(&input(
            &flaky,
            FlakeFinalStatus::Quarantined,
            Some(quarantine(&flaky, NOW + MAX_QUARANTINE_TTL_MS + 1)),
        ))),
        set(&["flake.quarantine_expiry_unbounded"])
    );
}

#[test]
fn quarantine_is_required_for_flakes_and_refused_for_stable_outcomes() {
    let flaky = flaky_attempts();
    for status in [FlakeFinalStatus::NeedsReview, FlakeFinalStatus::Quarantined] {
        assert_eq!(
            codes(&evaluate(&input(&flaky, status, None))),
            set(&["flake.quarantine_missing"])
        );
        assert!(evaluate(&input(&flaky, status, Some(live_quarantine(&flaky)))).is_empty());
    }

    // Stable outcomes carry no quarantine record.
    let stable_pass = vec![attempt(1, AttemptOutcome::Pass, SEED, &sha('a'))];
    assert!(evaluate(&input(&stable_pass, FlakeFinalStatus::Pass, None)).is_empty());
    assert_eq!(
        codes(&evaluate(&input(
            &stable_pass,
            FlakeFinalStatus::Pass,
            Some(live_quarantine(&stable_pass)),
        ))),
        set(&["flake.quarantine_unexpected"])
    );
}

#[test]
fn attempt_schema_order_and_case_bindings_fail_closed() {
    let mut drifted = attempt(1, AttemptOutcome::Fail, SEED, &sha('a'));
    drifted.schema = "kiana.quality-flake-attempt.v2".to_owned();
    assert_eq!(
        codes(&evaluate(&input(&[drifted], FlakeFinalStatus::Fail, None,))),
        set(&["flake.attempt_schema_invalid"])
    );

    // Attempt numbers must be sequential from one.
    let out_of_order = vec![attempt(2, AttemptOutcome::Fail, SEED, &sha('a'))];
    assert_eq!(
        codes(&evaluate(&input(
            &out_of_order,
            FlakeFinalStatus::Fail,
            None,
        ))),
        set(&["flake.attempt_order_invalid"])
    );
    let duplicated_number = vec![
        attempt(1, AttemptOutcome::Fail, SEED, &sha('a')),
        attempt(1, AttemptOutcome::Fail, SEED, &sha('b')),
    ];
    assert_eq!(
        codes(&evaluate(&input(
            &duplicated_number,
            FlakeFinalStatus::Fail,
            None,
        ))),
        set(&["flake.attempt_order_invalid"])
    );

    // Every attempt binds the envelope's bounded case identity.
    let other_case = vec![FlakeAttempt::new(
        1,
        "case:other",
        SEED,
        AttemptOutcome::Fail,
        sha('a'),
    )];
    assert_eq!(
        codes(&evaluate(&input(&other_case, FlakeFinalStatus::Fail, None))),
        set(&["flake.attempt_case_mismatch"])
    );
    let unbounded_case = vec![FlakeAttempt::new(
        1,
        "case has space",
        SEED,
        AttemptOutcome::Fail,
        sha('a'),
    )];
    assert_eq!(
        codes(&evaluate(&input(
            &unbounded_case,
            FlakeFinalStatus::Fail,
            None,
        ))),
        set(&["flake.attempt_case_mismatch"])
    );

    // An unbounded envelope identity is refused before its attempts can match it.
    let shared_bad_case = "case has space";
    let attempts = vec![FlakeAttempt::new(
        1,
        shared_bad_case,
        SEED,
        AttemptOutcome::Fail,
        sha('a'),
    )];
    let envelope =
        FlakeInput::new(shared_bad_case, NOW, attempts, FlakeFinalStatus::Fail).as_value();
    assert_eq!(
        codes(&evaluate(&envelope)),
        set(&["flake.case_invalid", "flake.attempt_case_mismatch"])
    );
}

#[test]
fn attempt_evidence_digests_must_be_strict_and_fresh() {
    for bad in ["a".repeat(64), "sha256:abc".to_owned(), sha('g')] {
        let attempts = vec![attempt(1, AttemptOutcome::Fail, SEED, &bad)];
        assert_eq!(
            codes(&evaluate(&input(&attempts, FlakeFinalStatus::Fail, None,))),
            set(&["flake.attempt_evidence_invalid"]),
            "digest must be strict sha256: {bad}"
        );
    }

    // A retry that replays the first attempt's evidence is not a rerun.
    let replayed = vec![
        attempt(1, AttemptOutcome::Fail, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('a')),
    ];
    assert_eq!(
        codes(&evaluate(&input(
            &replayed,
            FlakeFinalStatus::Quarantined,
            Some(live_quarantine(&replayed)),
        ))),
        set(&["flake.attempt_evidence_repeated"])
    );
}

#[test]
fn envelope_schema_attempts_time_and_unknown_fields_fail_closed() {
    let flaky = flaky_attempts();
    let mut drifted_schema = input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(live_quarantine(&flaky)),
    );
    drifted_schema["schema"] = json!("kiana.quality-flake-input.v2");
    assert!(evaluate_error(&drifted_schema).contains("flake_input_schema_invalid"));

    let empty = FlakeInput::new(CASE, NOW, Vec::new(), FlakeFinalStatus::Pass).as_value();
    assert!(evaluate_error(&empty).contains("flake_input_attempts_missing"));

    let three = vec![
        attempt(1, AttemptOutcome::Fail, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
        attempt(3, AttemptOutcome::Pass, SEED, &sha('c')),
    ];
    assert!(
        evaluate_error(&FlakeInput::new(CASE, NOW, three, FlakeFinalStatus::Pass).as_value())
            .contains("flake_attempt_limit_exceeded")
    );

    let unset_time =
        FlakeInput::new(CASE, 0, flaky.clone(), FlakeFinalStatus::NeedsReview).as_value();
    assert!(evaluate_error(&unset_time).contains("flake_input_time_invalid"));

    // Exactly one retry is accepted.
    assert!(FlakeClassifierEvaluator
        .evaluate(&input(
            &flaky,
            FlakeFinalStatus::NeedsReview,
            Some(live_quarantine(&flaky)),
        ))
        .is_ok());

    // Unknown fields, wrong types and unknown variants are refused by the strict decoder.
    let mut unknown_envelope = input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(live_quarantine(&flaky)),
    );
    unknown_envelope["unexpected"] = json!(true);
    assert!(FlakeClassifierEvaluator
        .evaluate(&unknown_envelope)
        .is_err());

    let mut unknown_attempt = input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(live_quarantine(&flaky)),
    );
    unknown_attempt["attempts"][0]["unexpected"] = json!(true);
    assert!(FlakeClassifierEvaluator.evaluate(&unknown_attempt).is_err());

    let mut wrong_type = input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(live_quarantine(&flaky)),
    );
    wrong_type["attempts"][0]["seed"] = json!("forty-two");
    assert!(FlakeClassifierEvaluator.evaluate(&wrong_type).is_err());

    let mut unknown_status = input(
        &flaky,
        FlakeFinalStatus::Quarantined,
        Some(live_quarantine(&flaky)),
    );
    unknown_status["final_status"] = json!("flaky");
    assert!(FlakeClassifierEvaluator.evaluate(&unknown_status).is_err());

    let infra = vec![attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
        .with_infra_class(InfraFailureClass::Runner)];
    let mut unknown_class = input(&infra, FlakeFinalStatus::Infra, None);
    unknown_class["attempts"][0]["infra_class"] = json!("cosmic");
    assert!(FlakeClassifierEvaluator.evaluate(&unknown_class).is_err());
}

#[test]
fn case_identity_must_be_bounded_and_redacted() {
    let attempts = vec![FlakeAttempt::new(
        1,
        SECRET_CASE,
        SEED,
        AttemptOutcome::Fail,
        sha('a'),
    )];
    let envelope = FlakeInput::new(SECRET_CASE, NOW, attempts, FlakeFinalStatus::Fail).as_value();
    let findings = evaluate(&envelope);
    assert_eq!(
        codes(&findings),
        set(&["flake.case_invalid", "flake.attempt_case_mismatch"])
    );
    let case_invalid = findings
        .iter()
        .find(|finding| finding.code == "flake.case_invalid")
        .expect("case identity finding");
    assert_eq!(case_invalid.evidence_ref, "flake:input");
    assert_eq!(case_invalid.actual["case_bounded"], json!(false));
    let serialized = serde_json::to_string(&findings).expect("serialize findings");
    assert!(
        !serialized.contains("SHOULD_NOT_LEAK_1234567890"),
        "raw secret escaped the case finding"
    );

    // The documented identity bound is inclusive.
    for (suffix, expected) in [
        (MAX_FLAKE_TEXT_BYTES, BTreeSet::new()),
        (
            MAX_FLAKE_TEXT_BYTES + 1,
            set(&["flake.case_invalid", "flake.attempt_case_mismatch"]),
        ),
    ] {
        let long_case = "c".repeat(suffix);
        let attempts = vec![FlakeAttempt::new(
            1,
            long_case.as_str(),
            SEED,
            AttemptOutcome::Pass,
            sha('a'),
        )];
        let envelope =
            FlakeInput::new(long_case.as_str(), NOW, attempts, FlakeFinalStatus::Pass).as_value();
        assert_eq!(codes(&evaluate(&envelope)), expected, "bound {suffix}");
    }
}

#[test]
fn classification_only_follows_attempt_evidence() {
    type ClassifyCase<'a> = (&'a [FlakeAttempt], FlakeClassification);

    let single_pass = [attempt(1, AttemptOutcome::Pass, SEED, &sha('a'))];
    let single_failure = [attempt(1, AttemptOutcome::Fail, SEED, &sha('a'))];
    let flaky = flaky_attempts();
    let stable = repeated_failure_attempts();
    let infra = [attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
        .with_infra_class(InfraFailureClass::Runner)];
    let out_of_scope = [attempt(1, AttemptOutcome::OutOfScope, SEED, &sha('a'))];
    let pass_then_fail = [
        attempt(1, AttemptOutcome::Pass, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Fail, SEED, &sha('b')),
    ];
    let infra_then_pass = [
        attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
            .with_infra_class(InfraFailureClass::Network),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
    ];
    let scope_then_pass = [
        attempt(1, AttemptOutcome::OutOfScope, SEED, &sha('a')),
        attempt(2, AttemptOutcome::Pass, SEED, &sha('b')),
    ];

    let cases: [ClassifyCase<'_>; 9] = [
        (single_pass.as_slice(), FlakeClassification::Pass),
        (single_failure.as_slice(), FlakeClassification::Failure),
        (flaky.as_slice(), FlakeClassification::Flake),
        (stable.as_slice(), FlakeClassification::Failure),
        (infra.as_slice(), FlakeClassification::Infra),
        (out_of_scope.as_slice(), FlakeClassification::OutOfScope),
        (
            pass_then_fail.as_slice(),
            FlakeClassification::RetryAfterPass,
        ),
        (infra_then_pass.as_slice(), FlakeClassification::Infra),
        (scope_then_pass.as_slice(), FlakeClassification::OutOfScope),
    ];

    for (attempts, expected) in cases {
        for status in [
            FlakeFinalStatus::Pass,
            FlakeFinalStatus::Fail,
            FlakeFinalStatus::Quarantined,
        ] {
            let envelope = FlakeInput::new(CASE, NOW, attempts.to_vec(), status);
            assert_eq!(
                classify_flake(&envelope),
                expected,
                "classification must not depend on the claimed status"
            );
        }
    }
}

#[test]
fn flake_contracts_round_trip_through_typed_input() {
    let attempts = flaky_attempts();
    let typed = FlakeInput::new(CASE, NOW, attempts.clone(), FlakeFinalStatus::Quarantined)
        .with_quarantine(live_quarantine(&attempts));
    let value = typed.as_value();

    let decoded: FlakeInput = serde_json::from_value(value.clone()).expect("typed input decodes");
    assert_eq!(decoded, typed);
    assert_eq!(decoded.as_value(), value);
    assert!(decoded.validate().is_ok());
    assert!(evaluate_flake(&typed)
        .expect("typed input evaluates")
        .is_empty());

    let record = live_quarantine(&attempts);
    let record_value = record.as_value();
    assert_eq!(
        serde_json::from_value::<FlakeQuarantine>(record_value).expect("record decodes"),
        record
    );

    let attempt_value = attempts[0].as_value();
    assert_eq!(attempt_value["schema"], json!(FLAKE_ATTEMPT_SCHEMA));

    // The deterministic digest binds identity, seed and outcome.
    let digest = flake_attempt_evidence_digest(&attempts);
    assert!(digest.starts_with("sha256:"));
    assert_eq!(digest.len(), 71);
    assert_eq!(digest, flake_attempt_evidence_digest(&attempts));
    assert_ne!(
        digest,
        flake_attempt_evidence_digest(&repeated_failure_attempts())
    );
    let mut reseeded = attempts.clone();
    reseeded[1].seed = SEED + 1;
    assert_ne!(digest, flake_attempt_evidence_digest(&reseeded));

    // A schema drift in the encoded envelope is refused before evaluation.
    let mut drifted = value;
    drifted["schema"] = json!("kiana.quality-flake-input.v2");
    assert!(serde_json::from_value::<FlakeInput>(drifted).is_err());
}

#[test]
fn flake_output_is_deterministic_evidence_bound_and_registry_wired() {
    // A deliberately mixed fixture: a drifted attempt schema, the flake shape and a missing
    // quarantine record, so findings span every evidence section.
    let mut drifted = attempt(1, AttemptOutcome::Fail, SEED, &sha('a'));
    drifted.schema = "kiana.quality-flake-attempt.v2".to_owned();
    let attempts = vec![drifted, attempt(2, AttemptOutcome::Pass, SEED, &sha('b'))];
    let value = input(&attempts, FlakeFinalStatus::Pass, None);

    let first = evaluate(&value);
    let second = evaluate(&value);
    assert_eq!(first, second, "evaluation must be deterministic");
    assert_eq!(
        canonical_findings(first.clone()).expect("canonical findings"),
        canonical_findings(second).expect("canonical findings")
    );
    assert_eq!(
        codes(&first),
        set(&[
            "flake.attempt_schema_invalid",
            "flake.flaky_counted_as_pass",
            "flake.quarantine_missing",
        ])
    );
    let refs: BTreeSet<&str> = first
        .iter()
        .map(|finding| finding.evidence_ref.as_str())
        .collect();
    assert_eq!(
        refs,
        BTreeSet::from(["flake:attempt:0", "flake:status", "flake:quarantine"])
    );

    let mut registry = EvaluatorRegistry::new();
    registry
        .register(FlakeClassifierEvaluator)
        .expect("register flake classifier");
    assert_eq!(
        registry.evaluator_ids(),
        vec![FLAKE_EVALUATOR_ID.to_owned()]
    );

    let run = registry.run(&value).expect("registry evaluation");
    assert_eq!(
        canonical_findings(run).expect("canonical findings"),
        canonical_findings(evaluate(&value)).expect("canonical findings")
    );

    let result = registry
        .evaluate_with_digest(&value)
        .expect("registry evaluation with digest");
    assert_eq!(
        result.findings_digest,
        digest_findings(&first).expect("findings digest")
    );
    assert_eq!(result.evaluator_ids, vec![FLAKE_EVALUATOR_ID.to_owned()]);
    assert_eq!(result.schema, "kiana.quality-evaluation-result.v1");
    assert_eq!(
        canonical_findings(result.findings).expect("canonical findings"),
        canonical_findings(evaluate(&value)).expect("canonical findings")
    );
}

#[test]
fn clean_evidence_stays_visible_across_every_outcome() {
    let flaky = flaky_attempts();
    let clean_cases = vec![
        (
            vec![attempt(1, AttemptOutcome::Pass, SEED, &sha('a'))],
            FlakeFinalStatus::Pass,
            None,
        ),
        (repeated_failure_attempts(), FlakeFinalStatus::Fail, None),
        (
            vec![attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
                .with_infra_class(InfraFailureClass::Runner)],
            FlakeFinalStatus::Infra,
            None,
        ),
        (
            vec![attempt(1, AttemptOutcome::OutOfScope, SEED, &sha('a'))],
            FlakeFinalStatus::OutOfScope,
            None,
        ),
        (
            vec![
                attempt(1, AttemptOutcome::Infra, SEED, &sha('a'))
                    .with_infra_class(InfraFailureClass::Network),
                attempt(2, AttemptOutcome::Fail, SEED, &sha('b')),
            ],
            FlakeFinalStatus::NeedsReview,
            None,
        ),
        (
            flaky.clone(),
            FlakeFinalStatus::NeedsReview,
            Some(live_quarantine(&flaky)),
        ),
        (
            flaky.clone(),
            FlakeFinalStatus::Quarantined,
            Some(live_quarantine(&flaky)),
        ),
    ];

    for (attempts, status, record) in clean_cases {
        let findings = evaluate(&input(&attempts, status, record));
        assert_eq!(
            codes(&findings),
            BTreeSet::new(),
            "clean evidence must stay clean under {status:?}: {findings:?}"
        );
    }

    // The schema constants used by the fixtures stay the documented ones.
    assert_eq!(FLAKE_INPUT_SCHEMA, "kiana.quality-flake-input.v1");
    assert_eq!(FLAKE_QUARANTINE_SCHEMA, "kiana.quality-flake-quarantine.v1");
    assert_eq!(FLAKE_EVALUATOR_ID, "flake-classifier");
}
