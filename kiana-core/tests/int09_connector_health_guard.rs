#[test]
fn int09_health_stays_read_only_server_owned_and_fixture_bound() {
    let domain = include_str!("../../kiana-domain/src/connectors.rs");
    let contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    let core = include_str!("../src/connectors.rs");
    let policy = include_str!("../../kiana-policy/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let query = include_str!("../../kiana-query/src/connector_health.rs");

    for marker in [
        "CONNECTOR_HEALTH_OPERATION",
        "ConnectorHealthStatus",
        "ConnectorHealthFact",
        "classify_connector_health_error",
        "CONNECTOR_HEALTH_EVENT_KIND",
        "connector.health_checked",
        "connector_health_binding_missing",
        "binding_id",
        "binding_snapshot",
        "probe_kind",
        "RiskLevel::ReadOnly",
        "connector_binding_scope_mismatch",
        "connector_binding_revoked",
        "operator_authorized",
        "load_fixture",
        "local_fixture_only",
        "external_provider_not_probed",
        "redacted_health_code",
        "append_idempotent_expected",
        "project_connector_health",
        "binding_revision_changed",
        "proof_level",
    ] {
        assert!(
            domain.contains(marker)
                || contracts.contains(marker)
                || core.contains(marker)
                || policy.contains(marker)
                || daemon.contains(marker)
                || query.contains(marker),
            "INT-09 marker missing: {marker}"
        );
    }

    let health_start = daemon
        .find("async fn handle_health")
        .expect("health handler");
    let health_end = daemon[health_start..]
        .find("async fn handle(")
        .map(|offset| health_start + offset)
        .expect("main handler");
    let health = &daemon[health_start..health_end];
    for forbidden in [
        "reqwest::",
        "hyper::",
        "TcpStream",
        "std::net",
        "CredentialStore",
        "SecretStore",
        "resolve_secret",
        "credential_bytes",
        "ConnectorEffect::Write",
        "ExternalSideEffect",
        "CapabilityBroker",
    ] {
        assert!(
            !health.contains(forbidden),
            "INT-09 health handler must not perform external or destructive work: {forbidden}"
        );
    }
    assert!(!domain.contains("EventStorePort"));
    assert!(!domain.contains("CapabilityBroker"));
}
