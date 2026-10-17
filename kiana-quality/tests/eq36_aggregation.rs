use kiana_quality::{
    canonical_findings, digest_findings, evaluate_aggregation, AggregateVerdict,
    AggregationEvaluator, AggregationInput, DeterministicEvaluator, DimensionAggregate,
    EvaluatorRegistry, Finding, AGGREGATION_EVALUATOR_ID, DIMENSION_AGGREGATE_SCHEMA,
    EVALUATION_RESULT_SCHEMA, MAX_AGGREGATION_DIMENSIONS, MAX_AGGREGATION_TEXT_BYTES,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn aggregate(
    dimension: &str,
    sample_count: u32,
    minimum_samples: u32,
    observed_milli: u64,
    verdict: AggregateVerdict,
) -> DimensionAggregate {
    DimensionAggregate::new(
        dimension,
        sample_count,
        minimum_samples,
        observed_milli,
        verdict,
    )
}

fn valid_dimension(dimension: &str) -> DimensionAggregate {
    aggregate(dimension, 30, 20, 1100, AggregateVerdict::Pass)
        .with_baseline_milli(1000)
        .with_absolute_limit_milli(1300)
        .with_relative_limit_milli(200)
        .with_confidence_interval(900, 1200, true)
}

fn input(dimensions: Vec<DimensionAggregate>) -> Value {
    AggregationInput::new(dimensions).as_value()
}

fn evaluate(input: &Value) -> Vec<Finding> {
    AggregationEvaluator
        .evaluate(input)
        .expect("aggregation fixture decodes")
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
fn insufficient_sample_is_blocked_or_needs_review() {
    // Card acceptance: with too few samples the only valid verdicts are `blocked` and
    // `needs_review`; a `pass` verdict is refused as a false pass.
    for verdict in [AggregateVerdict::Blocked, AggregateVerdict::NeedsReview] {
        let thin = aggregate("latency", 4, 10, 1100, verdict)
            .with_baseline_milli(1000)
            .with_absolute_limit_milli(1300)
            .with_relative_limit_milli(200)
            .with_confidence_interval(900, 1200, true);
        let findings = evaluate(&input(vec![thin]));
        assert_eq!(codes(&findings), set(&["aggregation.insufficient_sample"]));
        assert_eq!(findings[0].evidence_ref, "aggregation:dimension:0");
    }

    let pass = aggregate("latency", 4, 10, 1100, AggregateVerdict::Pass)
        .with_baseline_milli(1000)
        .with_absolute_limit_milli(1300)
        .with_relative_limit_milli(200)
        .with_confidence_interval(900, 1200, true);
    let findings = evaluate(&input(vec![pass]));
    assert_eq!(
        codes(&findings),
        set(&[
            "aggregation.insufficient_sample",
            "aggregation.false_pass",
            "aggregation.sample_verdict_invalid",
        ])
    );
    for finding in findings.iter().filter(|finding| {
        finding.code == "aggregation.false_pass"
            || finding.code == "aggregation.sample_verdict_invalid"
    }) {
        assert_eq!(finding.evidence_ref, "aggregation:verdict");
    }

    // `fail` is also not an accepted verdict for an insufficient sample, but it is not a pass.
    let fail = aggregate("latency", 4, 10, 1100, AggregateVerdict::Fail)
        .with_baseline_milli(1000)
        .with_absolute_limit_milli(1300)
        .with_relative_limit_milli(200)
        .with_confidence_interval(900, 1200, true);
    assert_eq!(
        codes(&evaluate(&input(vec![fail]))),
        set(&[
            "aggregation.insufficient_sample",
            "aggregation.sample_verdict_invalid",
        ])
    );
}

#[test]
fn sufficient_sample_with_explicit_thresholds_and_interval_is_valid() {
    assert!(evaluate(&input(vec![valid_dimension("latency")])).is_empty());

    // The documented boundaries are inclusive: sample count, absolute limit, relative delta and
    // both confidence bounds at the observed value.
    let boundary = aggregate("latency", 20, 20, 1200, AggregateVerdict::Pass)
        .with_baseline_milli(1000)
        .with_absolute_limit_milli(1200)
        .with_relative_limit_milli(200)
        .with_confidence_interval(1200, 1200, true);
    assert!(evaluate(&input(vec![boundary])).is_empty());

    // One explicit threshold is enough; confidence is only explicit when required.
    let absolute_only =
        aggregate("latency", 20, 20, 1100, AggregateVerdict::Pass).with_absolute_limit_milli(1300);
    assert!(evaluate(&input(vec![absolute_only])).is_empty());
}

#[test]
fn dimension_schema_drift_is_rejected() {
    let mut drifted = valid_dimension("latency");
    drifted.schema = "kiana.quality-dimension-aggregate.v2".to_owned();
    let findings = evaluate(&input(vec![drifted]));
    assert_eq!(
        codes(&findings),
        set(&[
            "aggregation.dimension_schema_invalid",
            "aggregation.false_pass"
        ])
    );
    let finding = findings
        .iter()
        .find(|finding| finding.code == "aggregation.dimension_schema_invalid")
        .expect("schema drift finding");
    assert_eq!(finding.expected, json!(DIMENSION_AGGREGATE_SCHEMA));
    assert_eq!(finding.evidence_ref, "aggregation:dimension:0");
}

#[test]
fn dimension_identity_must_be_bounded_and_unique() {
    let oversized = "d".repeat(MAX_AGGREGATION_TEXT_BYTES + 1);
    for name in [
        "",
        "latency score",
        "latency\nscore",
        "latency=",
        oversized.as_str(),
    ] {
        let findings = evaluate(&input(vec![valid_dimension(name)]));
        assert_eq!(
            codes(&findings),
            set(&["aggregation.dimension_invalid", "aggregation.false_pass"]),
            "identity must fail closed: {name:?}"
        );
        let finding = findings
            .iter()
            .find(|finding| finding.code == "aggregation.dimension_invalid")
            .expect("identity finding");
        assert_eq!(finding.actual["dimension_bounded"], json!(false));
        assert_eq!(finding.actual["dimension"], json!("invalid"));
    }

    // A bounded identity at the documented bound is accepted and stays visible.
    let bounded = "d".repeat(MAX_AGGREGATION_TEXT_BYTES);
    assert!(evaluate(&input(vec![valid_dimension(&bounded)])).is_empty());

    // Duplicate dimension evidence is refused instead of being silently deduplicated.
    let duplicate = evaluate(&input(vec![
        valid_dimension("latency"),
        valid_dimension("latency"),
    ]));
    assert_eq!(
        codes(&duplicate),
        set(&["aggregation.dimension_invalid", "aggregation.false_pass"])
    );
}

#[test]
fn minimum_sample_must_be_positive() {
    let zero =
        aggregate("latency", 30, 0, 1100, AggregateVerdict::Pass).with_absolute_limit_milli(1300);
    let findings = evaluate(&input(vec![zero]));
    assert_eq!(
        codes(&findings),
        set(&[
            "aggregation.minimum_sample_invalid",
            "aggregation.false_pass",
        ])
    );
    let finding = findings
        .iter()
        .find(|finding| finding.code == "aggregation.minimum_sample_invalid")
        .expect("minimum sample finding");
    assert_eq!(finding.expected, json!("positive_minimum_sample"));
}

#[test]
fn missing_threshold_policy_cannot_pass() {
    // No absolute and no relative limit: there is no explicit policy to pass against.
    let pass = aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass);
    assert_eq!(
        codes(&evaluate(&input(vec![pass]))),
        set(&["aggregation.threshold_missing", "aggregation.false_pass"])
    );

    // A review verdict keeps the missing policy visible without a false-pass refusal.
    let review = aggregate("latency", 30, 20, 1100, AggregateVerdict::NeedsReview);
    assert_eq!(
        codes(&evaluate(&input(vec![review]))),
        set(&["aggregation.threshold_missing"])
    );
}

#[test]
fn absolute_threshold_overage_is_blocking() {
    let over =
        aggregate("latency", 30, 20, 1301, AggregateVerdict::Pass).with_absolute_limit_milli(1300);
    assert_eq!(
        codes(&evaluate(&input(vec![over]))),
        set(&[
            "aggregation.absolute_threshold_exceeded",
            "aggregation.false_pass",
        ])
    );

    // A `fail` verdict keeps the overage visible without a false-pass finding.
    let failed =
        aggregate("latency", 30, 20, 1301, AggregateVerdict::Fail).with_absolute_limit_milli(1300);
    assert_eq!(
        codes(&evaluate(&input(vec![failed]))),
        set(&["aggregation.absolute_threshold_exceeded"])
    );
}

#[test]
fn relative_threshold_requires_a_positive_baseline() {
    let missing =
        aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass).with_relative_limit_milli(200);
    assert_eq!(
        codes(&evaluate(&input(vec![missing]))),
        set(&[
            "aggregation.relative_threshold_invalid",
            "aggregation.false_pass",
        ])
    );

    let zero_baseline = aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass)
        .with_baseline_milli(0)
        .with_relative_limit_milli(200);
    assert_eq!(
        codes(&evaluate(&input(vec![zero_baseline]))),
        set(&[
            "aggregation.relative_threshold_invalid",
            "aggregation.false_pass",
        ])
    );

    // A zero baseline with a zero observation has no relative drift to compute.
    let zero_pair = aggregate("latency", 30, 20, 0, AggregateVerdict::Pass)
        .with_baseline_milli(0)
        .with_relative_limit_milli(200);
    assert!(evaluate(&input(vec![zero_pair])).is_empty());
}

#[test]
fn relative_threshold_overage_is_blocking() {
    let at_limit = aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass)
        .with_baseline_milli(1000)
        .with_relative_limit_milli(100);
    assert!(evaluate(&input(vec![at_limit])).is_empty());

    let over = aggregate("latency", 30, 20, 1101, AggregateVerdict::Pass)
        .with_baseline_milli(1000)
        .with_relative_limit_milli(100);
    assert_eq!(
        codes(&evaluate(&input(vec![over]))),
        set(&[
            "aggregation.relative_threshold_exceeded",
            "aggregation.false_pass",
        ])
    );

    // Relative drift is absolute: an observation below the baseline is measured the same way.
    let below = aggregate("latency", 30, 20, 899, AggregateVerdict::Pass)
        .with_baseline_milli(1000)
        .with_relative_limit_milli(100);
    assert_eq!(
        codes(&evaluate(&input(vec![below]))),
        set(&[
            "aggregation.relative_threshold_exceeded",
            "aggregation.false_pass",
        ])
    );
}

#[test]
fn confidence_interval_is_required_explicitly() {
    // Confidence is optional by default: no interval, no finding.
    let optional =
        aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass).with_absolute_limit_milli(1300);
    assert!(evaluate(&input(vec![optional])).is_empty());

    let mut required_missing =
        aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass).with_absolute_limit_milli(1300);
    required_missing.confidence_required = true;
    assert_eq!(
        codes(&evaluate(&input(vec![required_missing]))),
        set(&["aggregation.confidence_missing", "aggregation.false_pass"])
    );

    // A half interval is still a missing interval when confidence is required.
    let mut half =
        aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass).with_absolute_limit_milli(1300);
    half.confidence_required = true;
    half.confidence_low_milli = Some(900);
    assert_eq!(
        codes(&evaluate(&input(vec![half]))),
        set(&["aggregation.confidence_missing", "aggregation.false_pass"])
    );
}

#[test]
fn inverted_or_partial_confidence_intervals_are_invalid() {
    let inverted = aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass)
        .with_absolute_limit_milli(1300)
        .with_confidence_interval(1400, 1000, true);
    assert_eq!(
        codes(&evaluate(&input(vec![inverted]))),
        set(&["aggregation.confidence_invalid", "aggregation.false_pass"])
    );

    // A half interval is malformed evidence even when confidence is not required.
    let mut partial =
        aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass).with_absolute_limit_milli(1300);
    partial.confidence_low_milli = Some(900);
    assert_eq!(
        codes(&evaluate(&input(vec![partial]))),
        set(&["aggregation.confidence_invalid", "aggregation.false_pass"])
    );

    // A complete, ordered, containing and optional interval stays clean.
    let optional = aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass)
        .with_absolute_limit_milli(1300)
        .with_confidence_interval(900, 1200, false);
    assert!(evaluate(&input(vec![optional])).is_empty());
}

#[test]
fn observed_outside_the_confidence_interval_is_invalid() {
    let outside = aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass)
        .with_baseline_milli(1000)
        .with_absolute_limit_milli(1300)
        .with_relative_limit_milli(200)
        .with_confidence_interval(900, 1000, true);
    assert_eq!(
        codes(&evaluate(&input(vec![outside]))),
        set(&["aggregation.confidence_invalid", "aggregation.false_pass"])
    );

    // The interval bounds are inclusive.
    for (low, high) in [(1100, 1200), (900, 1100)] {
        let at_bound = aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass)
            .with_baseline_milli(1000)
            .with_absolute_limit_milli(1300)
            .with_relative_limit_milli(200)
            .with_confidence_interval(low, high, true);
        assert!(
            evaluate(&input(vec![at_bound])).is_empty(),
            "interval [{low}, {high}] must contain 1100"
        );
    }
}

#[test]
fn false_pass_is_never_valid_for_blocking_evidence() {
    let cases = vec![
        (
            aggregate("latency", 4, 10, 1100, AggregateVerdict::Pass)
                .with_absolute_limit_milli(1300),
            "aggregation.insufficient_sample",
        ),
        (
            aggregate("latency", 30, 0, 1100, AggregateVerdict::Pass)
                .with_absolute_limit_milli(1300),
            "aggregation.minimum_sample_invalid",
        ),
        (
            aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass),
            "aggregation.threshold_missing",
        ),
        (
            aggregate("latency", 30, 20, 1400, AggregateVerdict::Pass)
                .with_absolute_limit_milli(1300),
            "aggregation.absolute_threshold_exceeded",
        ),
        (
            aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass)
                .with_relative_limit_milli(200),
            "aggregation.relative_threshold_invalid",
        ),
        (
            aggregate("latency", 30, 20, 1100, AggregateVerdict::Pass)
                .with_baseline_milli(1000)
                .with_relative_limit_milli(200)
                .with_confidence_interval(900, 1000, true),
            "aggregation.confidence_invalid",
        ),
    ];

    for (item, expected_code) in cases {
        let findings = evaluate(&input(vec![item]));
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "aggregation.false_pass"),
            "missing false pass for {expected_code}: {findings:?}"
        );
        assert!(
            findings.iter().any(|finding| finding.code == expected_code),
            "missing blocking finding {expected_code}: {findings:?}"
        );
        let false_pass = findings
            .iter()
            .find(|finding| finding.code == "aggregation.false_pass")
            .expect("false pass finding");
        assert_eq!(false_pass.evidence_ref, "aggregation:verdict");
    }

    // The same blocking evidence with a `blocked` verdict keeps the claim instead of a pass.
    let blocked = aggregate("latency", 30, 20, 1400, AggregateVerdict::Blocked)
        .with_absolute_limit_milli(1300);
    assert_eq!(
        codes(&evaluate(&input(vec![blocked]))),
        set(&["aggregation.absolute_threshold_exceeded"])
    );
}

#[test]
fn envelope_schema_dimension_limits_and_unknown_fields_fail_closed() {
    let mut wrong_schema = AggregationInput::new(vec![valid_dimension("latency")]).as_value();
    wrong_schema["schema"] = json!("kiana.quality-aggregation-input.v2");
    let error = AggregationEvaluator
        .evaluate(&wrong_schema)
        .expect_err("schema drift must fail closed");
    assert!(
        error
            .to_string()
            .contains("aggregation_input_schema_invalid"),
        "unexpected error: {error}"
    );

    // No dimension evidence at all cannot be a pass.
    let empty = AggregationInput::new(Vec::new()).as_value();
    let error = AggregationEvaluator
        .evaluate(&empty)
        .expect_err("empty evidence must fail closed");
    assert!(
        error
            .to_string()
            .contains("aggregation_input_dimensions_missing"),
        "unexpected error: {error}"
    );

    // Over the dimension bound is structural; exactly at the bound is accepted.
    let oversized: Vec<DimensionAggregate> = (0..=MAX_AGGREGATION_DIMENSIONS)
        .map(|index| valid_dimension(&format!("d{index}")))
        .collect();
    let error = AggregationEvaluator
        .evaluate(&AggregationInput::new(oversized).as_value())
        .expect_err("oversized dimension set must fail closed");
    assert!(
        error
            .to_string()
            .contains("aggregation_dimension_limit_exceeded"),
        "unexpected error: {error}"
    );

    let bounded: Vec<DimensionAggregate> = (0..MAX_AGGREGATION_DIMENSIONS)
        .map(|index| valid_dimension(&format!("d{index}")))
        .collect();
    assert!(AggregationEvaluator
        .evaluate(&AggregationInput::new(bounded).as_value())
        .is_ok());

    // Unknown fields on the envelope and on a dimension are refused by the strict decoder.
    let mut unknown_envelope = AggregationInput::new(vec![valid_dimension("latency")]).as_value();
    unknown_envelope["unexpected"] = json!(true);
    assert!(AggregationEvaluator.evaluate(&unknown_envelope).is_err());

    let mut unknown_dimension = AggregationInput::new(vec![valid_dimension("latency")]).as_value();
    unknown_dimension["dimensions"][0]["unexpected"] = json!(true);
    assert!(AggregationEvaluator.evaluate(&unknown_dimension).is_err());

    // Wrong field types are refused before evaluation as well.
    let mut wrong_type = AggregationInput::new(vec![valid_dimension("latency")]).as_value();
    wrong_type["dimensions"][0]["sample_count"] = json!("thirty");
    assert!(AggregationEvaluator.evaluate(&wrong_type).is_err());
}

#[test]
fn dimension_order_does_not_change_the_findings() {
    let mut latency = valid_dimension("latency");
    latency.observed_milli = 1400;
    let mut cost = valid_dimension("cost");
    cost.sample_count = 4;

    let forward = evaluate(&input(vec![latency.clone(), cost.clone()]));
    let backward = evaluate(&input(vec![cost.clone(), latency.clone()]));

    assert_eq!(
        canonical_findings(forward.clone()).expect("canonical findings"),
        canonical_findings(backward.clone()).expect("canonical findings")
    );

    // Evidence indices bind the sorted position, not the caller's array order: "cost" sorts
    // before "latency", so the overage always cites index 1.
    for findings in [forward, backward] {
        let overage = findings
            .iter()
            .find(|finding| finding.code == "aggregation.absolute_threshold_exceeded")
            .expect("overage finding");
        assert_eq!(overage.evidence_ref, "aggregation:dimension:1");
        let insufficient = findings
            .iter()
            .find(|finding| finding.code == "aggregation.insufficient_sample")
            .expect("insufficient finding");
        assert_eq!(insufficient.evidence_ref, "aggregation:dimension:0");
    }
}

#[test]
fn redaction_boundary_covers_caller_supplied_values() {
    let secret_shaped = "token=SHOULD_NOT_LEAK_1234567890";
    let item = aggregate(secret_shaped, 30, 20, 1100, AggregateVerdict::Pass)
        .with_absolute_limit_milli(1300);
    let findings = evaluate(&input(vec![item]));
    assert_eq!(
        codes(&findings),
        set(&["aggregation.dimension_invalid", "aggregation.false_pass"])
    );

    let finding = findings
        .iter()
        .find(|finding| finding.code == "aggregation.dimension_invalid")
        .expect("identity finding");
    assert_eq!(finding.actual["dimension"], json!("invalid"));
    assert_eq!(finding.actual["dimension_bounded"], json!(false));

    let serialized = serde_json::to_string(&findings).expect("serialize findings");
    assert!(
        !serialized.contains("SHOULD_NOT_LEAK_1234567890"),
        "raw secret escaped the finding boundary"
    );
    assert!(
        !serialized.contains("token="),
        "secret marker escaped the finding boundary"
    );

    // A bounded identity stays visible so a report can name the dimension it refused.
    let bounded = evaluate(&input(vec![aggregate(
        "latency",
        30,
        20,
        1400,
        AggregateVerdict::Blocked,
    )
    .with_absolute_limit_milli(1300)]));
    let finding = bounded
        .iter()
        .find(|finding| finding.code == "aggregation.absolute_threshold_exceeded")
        .expect("overage finding");
    assert_eq!(finding.actual["dimension"], json!("latency"));
    assert_eq!(finding.actual["dimension_bounded"], json!(true));
}

#[test]
fn aggregation_contracts_round_trip_through_typed_input() {
    let relative_only = aggregate("cost", 12, 10, 950, AggregateVerdict::NeedsReview)
        .with_baseline_milli(900)
        .with_relative_limit_milli(250);
    let typed = AggregationInput::new(vec![valid_dimension("latency"), relative_only.clone()]);
    let value = typed.as_value();
    assert_eq!(value["dimensions"][0]["absolute_limit_milli"], json!(1300));
    assert_eq!(value["dimensions"][1]["confidence_low_milli"], Value::Null);
    assert_eq!(
        value["dimensions"][1]["schema"],
        json!(DIMENSION_AGGREGATE_SCHEMA)
    );

    let decoded: AggregationInput =
        serde_json::from_value(value.clone()).expect("typed round trip decodes");
    assert_eq!(decoded, typed);

    let direct = evaluate_aggregation(&typed).expect("typed evaluation");
    assert_eq!(direct, evaluate(&value));

    // The typed builders only produce the documented schema.
    assert_eq!(
        relative_only.as_value()["schema"],
        json!(DIMENSION_AGGREGATE_SCHEMA)
    );
}

#[test]
fn aggregation_output_is_deterministic_evidence_bound_and_registry_wired() {
    let value = input(vec![valid_dimension("latency")]);
    assert_eq!(evaluate(&value), evaluate(&value));

    let failing = input(vec![aggregate(
        "latency",
        4,
        10,
        1400,
        AggregateVerdict::Pass,
    )
    .with_baseline_milli(1000)
    .with_absolute_limit_milli(1300)
    .with_relative_limit_milli(200)
    .with_confidence_interval(900, 1200, true)]);
    let first = evaluate(&failing);
    let second = evaluate(&failing);
    assert_eq!(first, second);
    assert_eq!(
        digest_findings(&first).expect("digest"),
        digest_findings(&second).expect("digest")
    );

    let found: BTreeSet<String> = codes(&first);
    for expected in [
        "aggregation.insufficient_sample",
        "aggregation.absolute_threshold_exceeded",
        "aggregation.relative_threshold_exceeded",
        "aggregation.confidence_invalid",
        "aggregation.false_pass",
        "aggregation.sample_verdict_invalid",
    ] {
        assert!(found.contains(expected), "missing finding {expected}");
    }
    for finding in &first {
        assert!(
            finding.evidence_ref == "aggregation:dimension:0"
                || finding.evidence_ref == "aggregation:verdict",
            "unexpected evidence ref: {}",
            finding.evidence_ref
        );
    }

    let mut registry = EvaluatorRegistry::new();
    registry
        .register(AggregationEvaluator)
        .expect("register aggregation evaluator");
    assert_eq!(
        registry.evaluator_ids(),
        vec![AGGREGATION_EVALUATOR_ID.to_owned()]
    );
    let result = registry
        .evaluate_with_digest(&failing)
        .expect("registry evaluation");
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert_eq!(
        result.findings_digest,
        digest_findings(&result.findings).expect("digest")
    );
    assert!(result.validate().is_ok());
    assert_eq!(
        result.findings,
        canonical_findings(first).expect("canonical findings")
    );
}

#[test]
fn sample_sufficiency_is_independent_of_threshold_and_confidence_evidence() {
    let sufficient = |verdict| {
        aggregate("latency", 30, 20, 1100, verdict)
            .with_baseline_milli(1000)
            .with_absolute_limit_milli(1300)
            .with_relative_limit_milli(200)
            .with_confidence_interval(900, 1200, true)
    };
    let insufficient = |verdict| {
        aggregate("latency", 19, 20, 1100, verdict)
            .with_baseline_milli(1000)
            .with_absolute_limit_milli(1300)
            .with_relative_limit_milli(200)
            .with_confidence_interval(900, 1200, true)
    };

    assert!(evaluate(&input(vec![sufficient(AggregateVerdict::Pass)])).is_empty());

    // Clean thresholds and a containing interval cannot compensate for a missing sample.
    let findings = evaluate(&input(vec![insufficient(AggregateVerdict::Pass)]));
    assert_eq!(
        codes(&findings),
        set(&[
            "aggregation.insufficient_sample",
            "aggregation.false_pass",
            "aggregation.sample_verdict_invalid",
        ])
    );

    // Raising the minimum (without touching the other policies) is what flips the verdict.
    let mut raised = sufficient(AggregateVerdict::NeedsReview);
    raised.minimum_samples = 31;
    assert_eq!(
        codes(&evaluate(&input(vec![raised]))),
        set(&["aggregation.insufficient_sample"])
    );
}
