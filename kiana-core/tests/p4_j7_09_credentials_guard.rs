#[test]
fn provider_credentials_and_endpoint_boundaries_are_server_owned() {
    let config = include_str!("../../kiana-provider/src/config.rs");
    let gateway = include_str!("../../kiana-provider/src/lib.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let daemon = include_str!("../../kiana-daemon/src/model_client.rs");
    for marker in [
        "model_credential_header_invalid",
        "model_credential_unavailable",
        "model_endpoint_credentials_or_query_denied",
        "model_endpoint_requires_tls_or_loopback",
        "credential_revision",
        "HeaderValue::from_str",
    ] {
        assert!(
            config.contains(marker),
            "credential/config marker missing: {marker}"
        );
    }
    assert!(gateway.contains("configuration_snapshot"));
    assert!(config.contains("Policy::none"));
    assert!(config.contains("redirect"));
    assert!(transport.contains("connection.endpoint"));
    for marker in [
        "ProviderGateway",
        "project_authority",
        "model_selection_conflict",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon endpoint boundary marker missing: {marker}"
        );
    }
}
