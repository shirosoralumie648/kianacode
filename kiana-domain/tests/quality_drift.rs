use kiana_domain::{
    evaluate_drift, DriftAlertId, DriftEvaluationInput, DriftReport, DriftThreshold, DriftVerdict,
    EventId, RouteDecision, DRIFT_ALERT_EVENT_KIND, DRIFT_INPUT_SCHEMA,
};
use serde_json::json;

const TARGET: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROUTE: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const GRANT: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn report(error: bool, turns: usize) -> DriftReport {
    let route = RouteDecision {
        schema: "kiana.route-decision.v1".to_owned(),
        provider_id: "fake".to_owned(),
        model_id: "model-a".to_owned(),
        model_profile: "default".to_owned(),
        prompt_hash: "prompt-v1".to_owned(),
        route_digest: "route-v1".to_owned(),
        configuration_revision: "config-v1".to_owned(),
        budget_schema: "budget-v1".to_owned(),
        runtime_version: "runtime-v1".to_owned(),
    };
    let mut report = DriftReport::default();
    for index in 0..turns {
        report
            .record(route.clone(), EventId::new(), 100 + index as u64, error)
            .unwrap();
    }
    report
}

fn threshold(minimum_samples: u64, max_error_rate_milli: Option<u64>) -> DriftThreshold {
    let mut threshold = DriftThreshold {
        schema: "kiana.quality-drift-threshold.v1".to_owned(),
        minimum_samples,
        max_error_rate_milli,
        max_mean_elapsed_ms: Some(1_000),
        threshold_digest: String::new(),
    };
    threshold.threshold_digest = threshold.digest();
    threshold
}

fn make_input(report: DriftReport, threshold: DriftThreshold) -> DriftEvaluationInput {
    DriftEvaluationInput {
        schema: DRIFT_INPUT_SCHEMA.to_owned(),
        target_digest: TARGET.to_owned(),
        report,
        threshold,
        route_digest_before: ROUTE.to_owned(),
        route_digest_after: ROUTE.to_owned(),
        grant_digest_before: GRANT.to_owned(),
        grant_digest_after: GRANT.to_owned(),
        source_cursor: 9,
        alert_id: DriftAlertId::new(),
    }
}

#[test]
fn drift_threshold_breach_emits_alert_without_authority_change() {
    let result = evaluate_drift(&make_input(report(true, 2), threshold(1, Some(100)))).unwrap();
    assert_eq!(result.metrics.verdict, DriftVerdict::Alerted);
    let event = result.event.expect("threshold breach should emit an event");
    assert_eq!(event.kind, DRIFT_ALERT_EVENT_KIND);
    assert!(!event.alert.authority_changes_applied);
    assert_eq!(
        event.alert.route_digest_before,
        event.alert.route_digest_after
    );
    assert_eq!(
        event.alert.grant_digest_before,
        event.alert.grant_digest_after
    );
    assert!(event.validate().is_ok());
}

#[test]
fn insufficient_samples_need_review_and_do_not_pass() {
    let result = evaluate_drift(&make_input(report(false, 1), threshold(2, Some(1_000)))).unwrap();
    assert_eq!(result.metrics.verdict, DriftVerdict::NeedsReview);
    assert!(result.event.is_none());
}

#[test]
fn route_or_grant_drift_is_rejected_before_alerting() {
    let mut input = make_input(report(true, 2), threshold(1, Some(100)));
    input.route_digest_after = TARGET.to_owned();
    assert_eq!(evaluate_drift(&input), Err("drift_input_invalid"));

    let mut input = make_input(report(true, 2), threshold(1, Some(100)));
    input.grant_digest_after = TARGET.to_owned();
    assert_eq!(evaluate_drift(&input), Err("drift_input_invalid"));
}

#[test]
fn drift_rejects_digest_and_unknown_field_tampering() {
    let mut input =
        serde_json::to_value(make_input(report(false, 2), threshold(1, Some(1_000)))).unwrap();
    input["threshold"]["threshold_digest"] = json!(TARGET);
    let tampered: DriftEvaluationInput = serde_json::from_value(input).unwrap();
    assert_eq!(evaluate_drift(&tampered), Err("drift_input_invalid"));

    let mut input =
        serde_json::to_value(make_input(report(false, 2), threshold(1, Some(1_000)))).unwrap();
    input["unexpected"] = json!(true);
    assert!(serde_json::from_value::<DriftEvaluationInput>(input).is_err());
}
