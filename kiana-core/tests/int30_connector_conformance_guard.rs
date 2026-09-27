#[test]
fn connector_conformance_report_is_evidence_only() {
    let domain = include_str!("../../kiana-domain/src/connector_conformance.rs");
    let core = include_str!("../src/connector_conformance.rs");
    for marker in [
        "LocalFixture",
        "StdioMcpFake",
        "HttpFake",
        "Replay",
        "Unknown",
        "Toctou",
        "effect_count",
        "dedupe_verified",
        "scope_fenced",
        "validate_connector_conformance_report",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "INT-30 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "ConnectorAdapter::invoke",
        "publish_live",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "INT-30 conformance executes effect: {forbidden}"
        );
    }
}
