//! EQ-46 failure-first behavior fixtures for version-bucket drift metrics, alerts and the
//! `drift.alerted` event payload.
//!
//! Every test names one rejected-first item from the card and asserts a structured error code
//! that is greppable in `kiana-domain/src/quality_drift.rs` or `kiana-domain/src/versioning.rs`.
//! Nothing here reads a live drift stream, appends an EventLog fact, switches a route, mutates a
//! grant or dispatches a provider: the module under test is a pure decision over supplied
//! digests, version buckets and counters, so these are contract fixtures and not runtime evidence.

use kiana_core::{evaluate_quality_drift, DRIFT_ALERT_COMMAND};
use kiana_domain::{
    DriftAlertEvent, DriftAlertId, DriftEvaluationInput, DriftReport, DriftThreshold, DriftVerdict,
    EventId, RouteDecision, DRIFT_ALERT_EVENT_KIND, DRIFT_ALERT_EVENT_SCHEMA, DRIFT_ALERT_SCHEMA,
    DRIFT_INPUT_SCHEMA, ROUTE_DECISION_SCHEMA,
};
use serde_json::json;

/// `DriftThreshold::validate` compares against this literal; it has no exported constant.
const THRESHOLD_SCHEMA: &str = "kiana.quality-drift-threshold.v1";

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

/// A bounded audit/version identity. It is not a provider request and not an execution permit.
fn route(model_id: &str) -> RouteDecision {
    RouteDecision {
        schema: ROUTE_DECISION_SCHEMA.to_owned(),
        provider_id: "provider:fixture".to_owned(),
        model_id: model_id.to_owned(),
        model_profile: "default".to_owned(),
        prompt_hash: "prompt-v1".to_owned(),
        route_digest: "route-v1".to_owned(),
        configuration_revision: "config-v1".to_owned(),
        budget_schema: "budget-v1".to_owned(),
        runtime_version: "runtime-v1".to_owned(),
    }
}

fn record_bucket(
    report: &mut DriftReport,
    model_id: &str,
    turns: u64,
    errors: u64,
    mean_elapsed_ms: u64,
) {
    for index in 0..turns {
        report
            .record(
                route(model_id),
                EventId::new(),
                mean_elapsed_ms,
                index < errors,
            )
            .expect("EQ-46 drift sample");
    }
}

/// One version bucket with 4 turns at 100ms each, so the derived error rate is `errors * 250`.
fn single_bucket_report(errors: u64) -> DriftReport {
    let mut report = DriftReport::default();
    record_bucket(&mut report, "model-a", 4, errors, 100);
    report
}

fn threshold(
    minimum_samples: u64,
    max_error_rate_milli: Option<u64>,
    max_mean_elapsed_ms: Option<u64>,
) -> DriftThreshold {
    let mut threshold = DriftThreshold {
        schema: THRESHOLD_SCHEMA.to_owned(),
        minimum_samples,
        max_error_rate_milli,
        max_mean_elapsed_ms,
        threshold_digest: String::new(),
    };
    threshold.threshold_digest = threshold.digest();
    threshold
}

fn input(report: DriftReport, threshold: DriftThreshold) -> DriftEvaluationInput {
    DriftEvaluationInput {
        schema: DRIFT_INPUT_SCHEMA.to_owned(),
        target_digest: digest('a'),
        report,
        threshold,
        route_digest_before: digest('b'),
        route_digest_after: digest('b'),
        grant_digest_before: digest('c'),
        grant_digest_after: digest('c'),
        source_cursor: 9,
        alert_id: DriftAlertId::new(),
    }
}

/// 2/4 turns erroring is 500 milli, which breaches the 400 milli limit below.
fn breached_input() -> DriftEvaluationInput {
    input(
        single_bucket_report(2),
        threshold(4, Some(400), Some(1_000)),
    )
}

/// Proves a claimed route switch is rejected with `drift_input_invalid` before any alert exists.
#[test]
fn route_digest_mismatch_is_rejected_before_any_alert() {
    let mut candidate = breached_input();
    candidate.route_digest_after = digest('e');
    assert_eq!(
        evaluate_quality_drift(&candidate),
        Err("drift_input_invalid")
    );
}

/// Proves a claimed grant mutation is rejected with `drift_input_invalid` before any alert exists.
#[test]
fn grant_digest_mismatch_is_rejected_before_any_alert() {
    let mut candidate = breached_input();
    candidate.grant_digest_after = digest('e');
    assert_eq!(
        evaluate_quality_drift(&candidate),
        Err("drift_input_invalid")
    );
}

/// Proves a bucket stored under a key other than its own `version_key` is rejected as
/// `drift_bucket_key_mismatch`, and that the adapter refuses the whole input as
/// `drift_input_invalid` instead of evaluating a mis-attributed version bucket.
#[test]
fn version_bucket_key_mismatch_is_rejected() {
    let mut report = single_bucket_report(2);
    let original_key = report
        .buckets
        .keys()
        .next()
        .expect("EQ-46 single version bucket")
        .clone();
    let bucket = report
        .buckets
        .remove(&original_key)
        .expect("EQ-46 relocatable bucket");
    report.buckets.insert(digest('d'), bucket);

    assert_eq!(report.validate(), Err("drift_bucket_key_mismatch"));
    assert_eq!(
        evaluate_quality_drift(&input(report, threshold(4, Some(400), Some(1_000)))),
        Err("drift_input_invalid")
    );
}

/// Proves a `version.drift` projection that claims an automatic model switch is rejected as
/// `drift_report_invalid`; drift evaluation has no route-selection authority to begin with.
#[test]
fn report_claiming_automatic_model_switch_is_rejected() {
    let mut report = single_bucket_report(2);
    report.automatic_model_switch = true;

    assert_eq!(report.validate(), Err("drift_report_invalid"));
    assert_eq!(
        evaluate_quality_drift(&input(report, threshold(4, Some(400), Some(1_000)))),
        Err("drift_input_invalid")
    );
}

/// Proves a report with no version buckets is rejected as `drift_samples_missing` instead of
/// silently evaluating to a healthy verdict.
#[test]
fn empty_report_is_rejected_without_samples() {
    let report = DriftReport::default();
    assert!(report.validate().is_ok());
    assert_eq!(
        evaluate_quality_drift(&input(report, threshold(4, Some(400), Some(1_000)))),
        Err("drift_samples_missing")
    );
}

/// Proves a zero minimum-sample threshold is rejected as `drift_threshold_invalid`, a threshold
/// with no limit at all as `drift_threshold_empty`, and both are refused by the adapter.
#[test]
fn invalid_or_empty_threshold_is_rejected() {
    let degenerate = threshold(0, Some(400), Some(1_000));
    assert_eq!(degenerate.validate(), Err("drift_threshold_invalid"));
    assert_eq!(
        evaluate_quality_drift(&input(single_bucket_report(2), degenerate)),
        Err("drift_input_invalid")
    );

    let limitless = threshold(4, None, None);
    assert_eq!(limitless.validate(), Err("drift_threshold_empty"));
    assert_eq!(
        evaluate_quality_drift(&input(single_bucket_report(2), limitless)),
        Err("drift_input_invalid")
    );
}

/// Proves a breaching bucket that is below `minimum_samples` yields `NeedsReview` with no alert
/// and no event, so insufficient evidence can never escalate into a `drift.alerted` fact.
#[test]
fn insufficient_samples_never_produce_an_alert() {
    let mut report = DriftReport::default();
    record_bucket(&mut report, "model-a", 1, 1, 100);
    let candidate = input(report, threshold(2, Some(400), Some(1_000)));

    let evaluation = evaluate_quality_drift(&candidate).expect("EQ-46 review verdict");
    assert_eq!(evaluation.metrics.verdict, DriftVerdict::NeedsReview);
    assert!(evaluation.alert.is_none());
    assert!(evaluation.event.is_none());
    assert!(evaluation.metrics.validate().is_ok());
}

/// Proves a tampered alert or event payload cannot present itself as publishable: flipping
/// `authority_changes_applied`, claiming a route switch, or renaming the event kind each fail
/// their structured validator even though the shape still deserializes.
#[test]
fn tampered_alert_or_event_payload_cannot_publish_itself() {
    let evaluation = evaluate_quality_drift(&breached_input()).expect("EQ-46 breach");
    let event = evaluation.event.expect("EQ-46 alert event");
    assert!(event.validate().is_ok());
    assert!(event.alert.validate().is_ok());

    let mut escalated = serde_json::to_value(&event).expect("EQ-46 event json");
    escalated["alert"]["authority_changes_applied"] = json!(true);
    let escalated: DriftAlertEvent = serde_json::from_value(escalated).expect("EQ-46 reserialized");
    assert_eq!(escalated.alert.validate(), Err("drift_alert_invalid"));
    assert_eq!(escalated.validate(), Err("drift_alert_event_invalid"));

    let mut rerouted = serde_json::to_value(&event).expect("EQ-46 event json");
    rerouted["alert"]["route_digest_after"] = json!(digest('e'));
    let rerouted: DriftAlertEvent = serde_json::from_value(rerouted).expect("EQ-46 reserialized");
    assert_eq!(rerouted.alert.validate(), Err("drift_alert_invalid"));
    assert_eq!(rerouted.validate(), Err("drift_alert_event_invalid"));

    let mut renamed = serde_json::to_value(&event).expect("EQ-46 event json");
    renamed["kind"] = json!("drift.remediated");
    let renamed: DriftAlertEvent = serde_json::from_value(renamed).expect("EQ-46 reserialized");
    assert_eq!(renamed.validate(), Err("drift_alert_event_invalid"));
}

/// Proves metrics keep one strictly ascending, de-duplicated bucket per version key: the real
/// two-version metrics validate, while duplicating a version bucket or reversing bucket order is
/// rejected as `drift_metrics_invalid`. The only mutation applied is the bucket list itself.
#[test]
fn cross_version_bucket_duplicate_or_reordered_metrics_are_rejected() {
    let key_a = route("model-a").version_key();
    let key_b = route("model-b").version_key();
    assert_ne!(
        key_a, key_b,
        "distinct models must yield distinct version keys"
    );

    let mut report = DriftReport::default();
    record_bucket(&mut report, "model-a", 4, 0, 100);
    record_bucket(&mut report, "model-b", 4, 4, 100);
    let evaluation = evaluate_quality_drift(&input(report, threshold(4, Some(400), Some(1_000))))
        .expect("EQ-46 two-version evaluation");

    let metrics = evaluation.metrics.clone();
    assert_eq!(metrics.buckets.len(), 2);
    assert!(metrics.buckets[0].version_key < metrics.buckets[1].version_key);
    assert!(metrics.validate().is_ok());
    // Only the breaching version bucket alerts, whichever order the version keys hash into.
    assert_eq!(evaluation.alert.expect("EQ-46 alert").version_key, key_b);

    let mut duplicated = metrics.clone();
    duplicated.buckets.push(duplicated.buckets[0].clone());
    assert_eq!(duplicated.validate(), Err("drift_metrics_invalid"));

    let mut reordered = metrics;
    reordered.buckets.swap(0, 1);
    assert_eq!(reordered.validate(), Err("drift_metrics_invalid"));
}

/// Proves a version bucket under both limits evaluates to `Healthy` with no alert and no event,
/// and that the source report still carries `automatic_model_switch=false`.
#[test]
fn below_threshold_metrics_stay_healthy_and_emit_no_alert() {
    let candidate = input(
        single_bucket_report(0),
        threshold(4, Some(400), Some(1_000)),
    );
    assert!(!candidate.report.automatic_model_switch);

    let evaluation = evaluate_quality_drift(&candidate).expect("EQ-46 healthy verdict");
    assert_eq!(evaluation.metrics.verdict, DriftVerdict::Healthy);
    assert!(evaluation.alert.is_none());
    assert!(evaluation.event.is_none());
    assert!(evaluation.metrics.validate().is_ok());
    assert_eq!(evaluation.metrics.buckets.len(), 1);
    assert_eq!(evaluation.metrics.buckets[0].error_rate_milli, 0);
    assert_eq!(evaluation.metrics.buckets[0].mean_elapsed_ms, 100);
}

/// The EQ-46 card acceptance: a `drift.alerted` event is produced for the breaching version
/// bucket, and it is evidence only — route and grant digests are identical before and after,
/// `authority_changes_applied` stays false, the report never auto-switches a model, and every
/// payload validates against its own contract.
#[test]
fn drift_alert_does_not_change_route_or_grant() {
    let candidate = breached_input();
    let evaluation = evaluate_quality_drift(&candidate).expect("EQ-46 breach evaluation");

    assert_eq!(evaluation.metrics.verdict, DriftVerdict::Alerted);
    assert!(evaluation.metrics.validate().is_ok());

    let alert = evaluation.alert.expect("EQ-46 alert evidence");
    assert_eq!(alert.schema, DRIFT_ALERT_SCHEMA);
    assert!(!alert.authority_changes_applied);
    assert_eq!(alert.route_digest_before, digest('b'));
    assert_eq!(alert.route_digest_after, alert.route_digest_before);
    assert_eq!(alert.grant_digest_before, digest('c'));
    assert_eq!(alert.grant_digest_after, alert.grant_digest_before);
    assert_eq!(alert.reason, "drift_threshold_exceeded");
    assert_eq!(alert.version_key, route("model-a").version_key());
    assert_eq!(alert.sample_count, 4);
    assert!(alert.validate().is_ok());

    let event = evaluation.event.expect("EQ-46 drift.alerted event");
    assert_eq!(event.schema, DRIFT_ALERT_EVENT_SCHEMA);
    assert_eq!(event.kind, DRIFT_ALERT_EVENT_KIND);
    assert_eq!(event.kind, DRIFT_ALERT_COMMAND);
    assert_eq!(event.source_cursor, 9);
    assert_eq!(event.alert, alert);
    assert!(event.validate().is_ok());

    assert!(!candidate.report.automatic_model_switch);
}
