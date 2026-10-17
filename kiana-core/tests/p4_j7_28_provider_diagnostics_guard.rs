#[test]
fn provider_diagnostics_projection_is_server_owned_and_deny_first() {
    let domain = include_str!("../../kiana-domain/src/provider_diagnostics.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let client = include_str!("../../kiana-client/src/provider_diagnostics.rs");
    let entrypoint = include_str!("../../kiana-entrypoints/src/provider_diagnostics.rs");
    let core = include_str!("../src/provider_diagnostics.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-28-provider-diagnostics-baseline.md");

    for marker in [
        "ProviderDiagnosticsSnapshot",
        "ProviderDiagnosticsCursor",
        "ProviderConnectionTestRequest",
        "ProviderConnectionTestPlan",
        "ProviderConfigCheckState",
        "ProviderDisplayMode",
        "ProviderUsageDiagnostic",
        "ProviderTerminalReplay",
        "ProviderSurface",
        "ProviderSurfaceProjection",
        "ProviderSettingsView",
        "ProviderUiAction",
        "ProviderUiActionDisposition",
        "ProviderStreamDisplay",
        "ProviderUpstreamEffect",
        "ProviderUpstreamAdmission",
        "PROVIDER_DIAGNOSTICS_PROJECTION_STALE",
        "PROVIDER_LIVE_EFFECT_ADMISSION_REQUIRED",
        "PROVIDER_UI_ACTION_STALE",
        "PROVIDER_STREAM_DISPLAY_SEQUENCE_GAP",
        "validate_for_epoch",
        "terminal_after",
        "connection_test_plan",
        "admit_provider_upstream_effect",
        "shared_digest",
        "request_attempts",
        "PROVIDER_DIAGNOSTICS_CONNECTION_TEST_EXPLICIT",
        "ProviderDiagnosticsClientState",
        "project_provider_diagnostics",
        "replay_provider_terminal",
        "snapshot",
        "cursor",
        "authority_epoch",
        "config_epoch",
        "native",
        "synthetic",
        "buffered",
        "queued",
        "retrying",
        "cancelling",
        "unknown",
    ] {
        assert!(
            domain.contains(marker)
                || protocol.contains(marker)
                || client.contains(marker)
                || entrypoint.contains(marker)
                || core.contains(marker)
                || baseline.contains(marker),
            "P4-J7-28 marker missing: {marker}"
        );
    }

    assert!(domain.contains("ProviderConfigSnapshot"));
    assert!(domain.contains("ModelCatalog"));
    assert!(domain.contains("settings_read_only_marker"));
    assert!(client.contains("connection_test_request"));
    assert!(entrypoint.contains("render_provider_diagnostics"));
    assert!(entrypoint.contains("render_provider_surface"));
    assert!(entrypoint.contains("render_provider_settings"));
    assert!(entrypoint.contains("render_provider_connection_test"));
    assert!(entrypoint.contains("render_provider_config_check"));
    assert!(entrypoint.contains("render_provider_terminal_replay"));
    assert!(client.contains("CursorGap"));
    assert!(client.contains("snapshot.cursor.sequence != sequence"));
    assert!(client.contains("begin_model_attempt"));
    assert!(client.contains("authorize_ui_action"));
    assert!(client.contains("settings_view"));
    assert!(domain.contains("self.sequence == 0"));
    assert!(domain.contains("self.expected_sequence == 0"));
    assert!(domain.contains("request_attempts > 1"));
    assert!(core.contains("StaleEpoch"));
    assert!(core.contains("terminal"));

    for fixture in [
        "three_surfaces_show_same_provider_diagnostics",
        "model_settings_view_does_not_trigger_inference",
        "stale_provider_ui_action_cannot_mutate_active_run",
        "late_subscriber_observes_terminal_from_receipt",
        "no_stream_collects_without_second_model_request",
        "web_reconnect_never_retries_model_request",
        "live_provider_effects_require_explicit_gateway_admission",
        "save_static_validate_and_live_verify_are_distinct_states",
    ] {
        assert!(
            baseline.contains(fixture),
            "P4-J7-28 baseline must name fixture: {fixture}"
        );
    }

    for forbidden in [
        "reqwest::Client",
        "ProviderGateway",
        "tokio::spawn",
        "std::net::TcpStream",
        "pub api_key",
        "pub authorization",
    ] {
        assert!(
            !domain.contains(forbidden),
            "diagnostics domain must not contain provider effect or secret field: {forbidden}"
        );
        assert!(
            !entrypoint.contains(forbidden),
            "diagnostics entrypoint must not contain provider effect or secret field: {forbidden}"
        );
    }
}
