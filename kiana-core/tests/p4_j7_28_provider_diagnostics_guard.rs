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
        "ProviderConfigCheckState",
        "ProviderDisplayMode",
        "ProviderUsageDiagnostic",
        "ProviderTerminalReplay",
        "PROVIDER_DIAGNOSTICS_PROJECTION_STALE",
        "validate_for_epoch",
        "terminal_after",
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
    assert!(entrypoint.contains("render_provider_terminal_replay"));
    assert!(client.contains("CursorGap"));
    assert!(client.contains("snapshot.cursor.sequence != sequence"));
    assert!(domain.contains("self.sequence == 0"));
    assert!(core.contains("StaleEpoch"));
    assert!(core.contains("terminal"));

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
