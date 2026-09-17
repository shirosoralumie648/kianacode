use kiana_domain::{DriftReport, RouteDecision, DRIFT_REPORT_SCHEMA};
use serde_json::json;

#[test]
fn drift_report_is_bucketed_by_version() {
    let base = json!({
        "provider_id": "fake",
        "model_id": "fake-coder",
        "model_profile": "builder",
        "prompt_hash": "sha256:prompt-v1",
        "route_digest": "sha256:route-v1",
        "configuration_revision": "config-v1",
        "budget": {"schema": "kiana.runtime-budget.v1"}
    });
    let mut changed = base.clone();
    changed["model_profile"] = json!("reviewer");

    let first = RouteDecision::from_model_turn(&base, "0.1.0");
    let second = RouteDecision::from_model_turn(&changed, "0.1.0");
    assert_ne!(first.version_key(), second.version_key());
    assert_eq!(first.schema, "kiana.route-decision.v1");
    first.validate().expect("route identity validates");

    let mut report = DriftReport::default();
    let first_event = kiana_domain::EventId::new();
    let second_event = kiana_domain::EventId::new();
    report
        .record(first.clone(), first_event, 12, false)
        .expect("first turn records");
    report
        .record(first, second_event, 8, true)
        .expect("same version shares a bucket");
    report
        .record(second, kiana_domain::EventId::new(), 4, false)
        .expect("profile change creates a new bucket");
    report.validate().expect("report validates");

    assert_eq!(report.schema, DRIFT_REPORT_SCHEMA);
    assert_eq!(report.buckets.len(), 2);
    assert_eq!(
        report
            .buckets
            .values()
            .map(|bucket| bucket.turns)
            .sum::<u64>(),
        3
    );
    assert_eq!(
        report
            .buckets
            .values()
            .map(|bucket| bucket.errors)
            .sum::<u64>(),
        1
    );

    let encoded = serde_json::to_value(&report).expect("strict report serializes");
    let decoded: DriftReport = serde_json::from_value(encoded.clone()).expect("round trip");
    assert_eq!(decoded, report);
    let mut unknown = encoded;
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<DriftReport>(unknown).is_err());

    let mut automatic = report;
    automatic.automatic_model_switch = true;
    assert_eq!(automatic.validate(), Err("drift_report_invalid"));
}
