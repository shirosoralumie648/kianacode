#[test]
fn dep18_incident_is_ordered_evidence_bound_and_effect_free() {
    let domain = include_str!("../../kiana-domain/src/deployment_incident.rs");
    let core = include_str!("../src/deployment_incident.rs");
    for marker in [
        "IncidentPhase",
        "RunbookRef",
        "AlertRoute",
        "RunbookEvidence",
        "IncidentInput",
        "IncidentReport",
        "incident_close_requires_verified",
        "incident_phase_deadline_expired",
        "auto_compensation",
        "evaluate_incident",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-18 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process",
        "EventStore::append",
        "CapabilityBroker",
        "KianaHarness",
        "ProviderGateway",
        "tokio::",
        "send_alert",
        "compensate",
        "grant_authority",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-18 incident crossed effect boundary: {forbidden}"
        );
    }
}
