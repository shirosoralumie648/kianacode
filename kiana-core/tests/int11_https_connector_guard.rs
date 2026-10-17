#[test]
fn int11_https_boundary_is_deny_first_and_pure_before_dispatch() {
    let domain = include_str!("../../kiana-domain/src/connector_https.rs");
    let ports = include_str!("../../kiana-ports/src/connector_https.rs");
    let connector = include_str!("../../kiana-ports/src/connector.rs");
    let core = include_str!("../src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/int11_https_connector.rs");
    let ports_fixtures = include_str!("../../kiana-ports/tests/int11_https_connector.rs");

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
        // Deny vocabulary and bounds that the fixture matrix has to keep exercising.
        "connector_https_url_userinfo_denied",
        "connector_https_non_https_denied",
        "connector_https_metadata_address_denied",
        "connector_https_egress_address_not_allowlisted",
        "connector_https_redirect_origin_denied",
        "connector_https_proxy_denied",
        "connector_https_proxy_origin_invalid",
        "CONNECTOR_HTTPS_MAX_REDIRECTS",
        "CONNECTOR_HTTPS_MAX_ADDRESSES",
        "literal_ip",
    ] {
        assert!(
            domain.contains(marker),
            "INT-11 domain marker missing: {marker}"
        );
    }
    for marker in [
        "ConnectorHttpsRequest",
        "ConnectorHttpsTransport",
        "ConnectorHttpsTransportCapabilities",
        "send_checked",
        "validate_for_binding",
        "validate_receipt_for_permit",
        "proxy_origin",
        "redirects",
        "connector_https_transport_boundary_unsupported",
        "connector_https_proxy_transport_unsupported",
        "connector_https_request_schema_invalid",
        "connector_https_redirect_limit_exceeded",
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

    // The acceptance fixtures must keep pinning the deny matrix, the egress/SSRF allowlists and
    // the pinned-origin success path; removing one of these obligations has to break this guard.
    for fixture_marker in [
        "endpoint_rejects_userinfo_non_https_query_fragment_and_origin_drift",
        "resolution_rejects_ssrf_metadata_and_unallowlisted_addresses",
        "redirect_and_proxy_are_pinned_and_structured",
        "policy_serialization_is_strict_and_digest_bound",
        "policy_bounds_and_unknown_fields_fail_closed",
        "dns_observation_is_sorted_bounded_and_policy_bound",
        "ssrf_and_metadata_addresses_fail_closed_for_every_class",
        "literal_ip_endpoints_require_exactly_the_observed_address",
        "endpoint_host_allowlist_is_exact_and_has_no_wildcards",
        "proxy_selection_is_explicit_and_allowlisted",
        "error_classification_is_stable_redaction_safe_and_serde_bound",
    ] {
        assert!(
            domain_fixtures.contains(fixture_marker),
            "INT-11 domain fixture obligation missing: {fixture_marker}"
        );
    }
    for fixture_marker in [
        "fake_https_transport_only_dispatches_pinned_origin",
        "forged_credential_endpoint_binding_is_rejected_before_dispatch",
        "proxy_or_cross_origin_redirect_is_rejected_before_fake_dispatch",
        "default_capabilities_and_send_deny_without_any_dispatch",
        "missing_security_capability_denies_before_the_implementation_runs",
        "request_deny_matrix_never_reaches_the_transport",
        "same_origin_redirect_chain_within_bound_dispatches_once",
        "explicit_proxy_requires_an_allowlisted_origin_and_a_proxy_capable_transport",
        "receipt_that_is_not_bound_to_the_permit_is_refused_after_dispatch",
        "request_contract_rejects_unknown_fields_and_absent_proxy_is_not_serialized",
    ] {
        assert!(
            ports_fixtures.contains(fixture_marker),
            "INT-11 ports fixture obligation missing: {fixture_marker}"
        );
    }
    assert!(
        !domain_fixtures.contains("#[ignore]") && !ports_fixtures.contains("#[ignore]"),
        "INT-11 fixtures must not be ignored"
    );
    assert!(
        ports_fixtures.contains("expect(\"send count\"), 0"),
        "INT-11 fixtures must prove the fake transport was never called"
    );

    for (name, source) in [("domain", domain), ("ports", ports)] {
        for forbidden in [
            "reqwest",
            "TcpStream",
            "ToSocketAddrs",
            "std::net::TcpStream",
            "std::env",
            "tokio::net",
            "EventStorePort",
            "ApprovalStorePort",
            "CapabilityBroker",
            "ProviderGateway",
            "unsafe",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
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
    // The HTTPS transport stays below the ControlPlane: neither the core routing nor the daemon
    // adapter selector may name it, so endpoint text/config cannot select a live transport.
    assert!(!core.contains("ConnectorHttps"));
    assert!(!daemon.contains("ConnectorHttps"));
    assert!(!daemon.contains("ConnectorHttpsTransport"));
}

#[test]
fn int11_checked_wrapper_validates_before_the_single_send_call() {
    let ports = include_str!("../../kiana-ports/src/connector_https.rs");
    assert_eq!(
        ports.matches("self.send(").count(),
        1,
        "the HTTPS port must have exactly one dispatch site"
    );
    let wrapper = ports
        .split("async fn send_checked")
        .nth(1)
        .expect("checked wrapper");
    let capabilities = wrapper
        .find("capabilities.validate()?")
        .expect("capability gate");
    let request_check = wrapper.find("request.validate()?").expect("request gate");
    let dispatch = wrapper
        .find("self.send(request).await?")
        .expect("dispatch call");
    let receipt_check = wrapper
        .find("validate_receipt_for_permit")
        .expect("receipt binding gate");
    assert!(
        capabilities < request_check,
        "the capability claim must be reviewed before the request gate"
    );
    assert!(
        request_check < dispatch,
        "the permit/endpoint/DNS/proxy/redirect checks must run before dispatch"
    );
    assert!(
        dispatch < receipt_check,
        "the returned receipt must be re-bound to the permit after dispatch"
    );
}

#[test]
fn int11_fixtures_are_wired_into_the_manual_lane() {
    let workflow = include_str!("../../.github/workflows/int11-https-connector.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int11_https_connector --locked -- --test-threads=1",
        "cargo test -p kiana-ports --test int11_https_connector --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int11_https_connector_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test int11_https_connector --no-run --locked",
        "cargo test -p kiana-ports --test int11_https_connector --no-run --locked",
        "cargo test -p kiana-core --test int11_https_connector_guard --no-run --locked",
    ] {
        assert!(
            workflow.contains(marker),
            "INT-11 lane marker missing: {marker}"
        );
    }
    for forbidden in ["continue-on-error", "#[ignore]"] {
        assert!(
            !workflow.contains(forbidden),
            "INT-11 lane must not contain {forbidden}"
        );
    }
}
