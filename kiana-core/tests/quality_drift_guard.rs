#[test]
fn drift_alert_is_evidence_only_and_keeps_route_and_grant_fenced() {
    let domain = include_str!("../../kiana-domain/src/quality_drift.rs");
    let core = include_str!("../src/quality_drift.rs");
    for marker in [
        "DriftThreshold",
        "DriftMetricBucket",
        "DriftMetrics",
        "DriftAlert",
        "DriftAlertEvent",
        "DRIFT_ALERT_EVENT_KIND",
        "minimum_samples",
        "authority_changes_applied",
        "route_digest_before",
        "grant_digest_before",
        "evaluate_quality_drift",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "EQ-46 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ModelClient",
        "std::process::Command",
        "EventStore",
        "route.switch",
        "grant.update",
        "automatic_model_switch = true",
        "authority_changes_applied: true",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "EQ-46 route/grant/effect marker present: {forbidden}"
        );
    }
}
