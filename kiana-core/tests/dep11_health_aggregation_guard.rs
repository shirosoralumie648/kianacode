#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn dep11_health_aggregation_is_projection_only() {
    let domain = source_guard::production_source(include_str!(
        "../../kiana-domain/src/health_aggregation.rs"
    ));
    let core = source_guard::production_source(include_str!("../src/health_aggregation.rs"));
    let daemon = source_guard::production_source(include_str!(
        "../../kiana-daemon/src/health_aggregation.rs"
    ));
    for marker in [
        "HealthAggregationInput",
        "HealthAggregationReport",
        "HealthSnapshot",
        "HealthProbeKind::Readiness",
        "HealthProbeKind::Drain",
        "HealthProbeKind::Maintenance",
        "provider_health_unverified",
        "projection_lag_or_cursor_ahead",
        "lease_conflict",
        "admission_allowed",
        "aggregate_health",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "DEP-11 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "tokio::",
        "EventStore",
        "CapabilityBroker",
        "ProviderGateway",
        "KianaHarness",
        "http::StatusCode",
        "StatusCode::OK",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden) && !daemon.contains(forbidden),
            "DEP-11 health path crossed an effect/transport boundary: {forbidden}"
        );
    }
}
