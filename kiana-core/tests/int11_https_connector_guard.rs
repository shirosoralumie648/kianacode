#[test]
fn int11_https_boundary_is_deny_first_and_pure_before_dispatch() {
    let domain = include_str!("../../kiana-domain/src/connector_https.rs");
    let ports = include_str!("../../kiana-ports/src/connector_https.rs");
    let connector = include_str!("../../kiana-ports/src/connector.rs");
    let core = include_str!("../src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");

    for marker in [
        "ConnectorHttpsPolicy",
        "CONNECTOR_HTTPS_POLICY_SCHEMA",
        "ConnectorHttpsErrorCode",
        "UrlUserinfoDenied",
        "NonHttpsDenied",
        "RedirectOriginDenied",
        "LocalAddressDenied",
        "MetadataAddressDenied",
        "EgressAddressNotAllowlisted",
        "allowed_egress_hosts",
        "allowed_egress_addresses",
        "pinned_origin",
        "observe_resolution",
    ] {
        assert!(
            domain.contains(marker),
            "INT-11 domain marker missing: {marker}"
        );
    }
    for marker in [
        "ConnectorHttpsRequest",
        "ConnectorHttpsTransport",
        "send_checked",
        "validate_for_binding",
        "proxy_origin",
        "redirects",
        "connector_https_transport_boundary_unsupported",
        "connector_https_proxy_transport_unsupported",
    ] {
        assert!(
            ports.contains(marker),
            "INT-11 port marker missing: {marker}"
        );
    }
    for marker in [
        "validate_for_binding",
        "validate_for(",
        "effect_target_digest",
        "connector_credential_audience",
        "CONNECTOR_CREDENTIAL_PURPOSE",
    ] {
        assert!(
            connector.contains(marker),
            "INT-11 connector binding marker missing: {marker}"
        );
    }
    for marker in ["ControlPlane", "connector", "project_trusted"] {
        assert!(
            core.contains(marker),
            "INT-11 core route marker missing: {marker}"
        );
    }
    for marker in ["local_fixture_only", "external_effect_performed"] {
        assert!(
            daemon.contains(marker),
            "INT-11 daemon compatibility marker missing: {marker}"
        );
    }

    for (name, source) in [("domain", domain), ("ports", ports)] {
        for forbidden in [
            "reqwest::Client",
            "std::net::ToSocketAddrs",
            "std::env::var",
            "tokio::net",
            "EventStorePort",
            "ApprovalStorePort",
        ] {
            assert!(
                !source.contains(forbidden),
                "INT-11 {name} boundary performs hidden authority/network I/O: {forbidden}"
            );
        }
    }
}

#[test]
fn int11_transport_cannot_be_selected_by_ambient_proxy_or_project_text() {
    let ports = include_str!("../../kiana-ports/src/connector_https.rs");
    let core = include_str!("../src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    for forbidden in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"] {
        assert!(
            !ports.contains(forbidden),
            "ambient proxy leaked into port: {forbidden}"
        );
    }
    assert!(!core.contains("base_url"));
    assert!(!daemon.contains("reqwest::Client"));
    assert!(!daemon.contains("ToSocketAddrs"));
}
