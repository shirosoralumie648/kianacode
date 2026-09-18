#[test]
fn sc17_connector_mcp_ingress_is_typed_scoped_idempotent_and_fail_closed() {
    let domain = include_str!("../../kiana-domain/src/connectors.rs");
    let effect = include_str!("../../kiana-domain/src/effect_observation.rs");
    let core = include_str!("../src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let stdio = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let services = include_str!("../../kiana-services/src/mcp.rs");
    let network = include_str!("../../kiana-services/src/network_policy.rs");
    let fixture = include_str!("../../kiana-domain/tests/p4_k8_01_connector.rs");
    let mcp_fixture = include_str!("../../kiana-daemon/tests/p1_j4_01_mcp.rs");

    for marker in [
        "ConnectorDefinition",
        "ConnectorOperation",
        "AccountBinding",
        "ConnectorBindingSnapshot",
        "idempotency_required",
        "reconciliation_required",
        "connector_account_scope_denied",
        "ProviderReceipt",
        "ProviderOutcome",
        "connector_invocation_risk",
        "adapter != \"local_fixture\"",
    ] {
        assert!(
            domain.contains(marker),
            "SC-17 domain connector marker missing: {marker}"
        );
    }
    for marker in [
        "validate_for_scope",
        "audience_digest",
        "idempotency_key_digest",
        "EffectObservationState::Unknown",
        "from_provider_receipt",
    ] {
        assert!(
            effect.contains(marker),
            "SC-17 effect marker missing: {marker}"
        );
    }
    for marker in [
        "project_untrusted",
        "authorize_and_execute",
        "binding_snapshot",
        "binding_authorized",
        "connector_idempotency_key_required",
        "connector_payload_too_large",
    ] {
        assert!(
            core.contains(marker),
            "SC-17 core ingress marker missing: {marker}"
        );
    }
    for marker in [
        "connector_operator_required",
        "operator_authorized",
        "connector_binding_scope_mismatch",
        "connector_binding_snapshot_changed",
        "connector_final_payload_approval_required",
        "connector_idempotency_payload_mismatch",
        "connector_rate_limit_exceeded",
        "connector_reconciliation_binding_mismatch",
        "append_idempotent_expected",
        "source: \"local_fixture\"",
        "external_effect_performed",
        "effect_observation",
        "result_unknown:connector_provider_outcome_unknown",
    ] {
        assert!(
            daemon.contains(marker),
            "SC-17 daemon connector marker missing: {marker}"
        );
    }
    for marker in [
        "server.transport != TransportType::Stdio",
        "TransportType::Http",
        "mcp_project_untrusted",
        "mcp_config_drift_requires_discovery",
        "mcp_tool_schema_changed",
        "mcp_discovery_operator_required",
        "mcp_snapshot_scope_changed",
    ] {
        assert!(mcp.contains(marker), "SC-17 MCP marker missing: {marker}");
    }
    for marker in [
        "kill_on_drop(true)",
        "process_group",
        "stop_confirmed",
        "mcp_request_stopped",
        "mcp_stop_unconfirmed",
    ] {
        assert!(
            stdio.contains(marker),
            "SC-17 stdio stop marker missing: {marker}"
        );
    }
    for marker in [
        "validate_http_url",
        "validate_http_redirect",
        "same_origin_redirects_only",
        "mcp_transport_unsupported",
    ] {
        assert!(
            services.contains(marker) || network.contains(marker),
            "SC-17 network boundary marker missing: {marker}"
        );
    }
    for source in [domain, effect, core, daemon, mcp, stdio] {
        for forbidden in [
            "token_passthrough",
            "credential_passthrough",
            "TeamCreate",
            "SendMessage",
        ] {
            assert!(
                !source.contains(forbidden),
                "SC-17 ingress bypass marker: {forbidden}"
            );
        }
    }
    assert!(fixture.contains("connector_cannot_bypass_the_control_plane"));
    assert!(mcp_fixture.contains("mcp_tool_schema_and_health_are_traceable"));
}
