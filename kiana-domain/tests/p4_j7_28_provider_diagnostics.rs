use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn route() -> ModelRoute {
    ModelRoute {
        provider_id: "provider-fixture".to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: "connection-fixture".to_owned(),
        model_id: "model-fixture".to_owned(),
        profile: "default".to_owned(),
        configuration_revision: "config-rev-1".to_owned(),
        streaming: true,
    }
}

fn capabilities() -> ModelCapabilities {
    ModelCapabilities {
        tools: CapabilitySupport::Supported,
        streaming: CapabilitySupport::Supported,
        structured_output: CapabilitySupport::Unknown,
        images: CapabilitySupport::Unsupported,
        reasoning_replay: CapabilitySupport::Unknown,
        context_window: 16_000,
        max_output: 2_000,
        source: "fixture".to_owned(),
        revision: "cap-rev-1".to_owned(),
    }
}

fn configuration() -> ProviderConfigSnapshot {
    ProviderConfigSnapshot::new(
        ProviderSelectionMode::Cassette,
        vec![ProviderProfileSnapshot::new(
            route(),
            capabilities(),
            Some(digest('k')),
            ProviderConfigSource::Explicit,
        )
        .expect("profile")],
    )
    .expect("configuration")
}

fn catalog() -> ModelCatalog {
    ModelCatalog::new(vec![ModelCatalogEntry {
        schema: MODEL_CATALOG_ENTRY_SCHEMA.to_owned(),
        provider_id: "provider-fixture".to_owned(),
        connection_id: "connection-fixture".to_owned(),
        model_id: "model-fixture".to_owned(),
        capabilities: capabilities(),
        source: ModelCatalogSource::Configured,
        catalog_revision: "catalog-rev-1".to_owned(),
        expires_at_unix_ms: None,
    }])
    .expect("catalog")
}

fn entry(
    status: ProviderDiagnosticStatus,
    usage: ProviderUsageDiagnostic,
) -> ProviderDiagnosticEntry {
    ProviderDiagnosticEntry::new(
        "provider-fixture",
        "connection-fixture",
        "default",
        "model-fixture",
        ModelProtocol::OpenAiChat,
        capabilities(),
        ProviderDisplayMode::Buffered,
        ProviderConfigCheckState::StaticValidated,
        status,
        usage,
    )
    .expect("diagnostic entry")
}

fn snapshot() -> ProviderDiagnosticsSnapshot {
    let configuration = configuration();
    let check = ProviderConfigCheck::new(
        ProviderConfigCheckState::StaticValidated,
        configuration.snapshot_digest.clone(),
        1,
        None,
    )
    .expect("config check");
    ProviderDiagnosticsSnapshot::new(
        7,
        11,
        configuration,
        catalog(),
        check,
        vec![entry(
            ProviderDiagnosticStatus::Ready,
            ProviderUsageDiagnostic::Known {
                usage: ModelUsage {
                    input_tokens: 12,
                    output_tokens: 8,
                },
            },
        )],
        None,
    )
    .expect("snapshot")
}

#[test]
fn snapshot_binds_catalog_config_and_cursor_without_secrets() {
    let snapshot = snapshot();
    snapshot.validate().expect("valid snapshot");
    let encoded = serde_json::to_string(&snapshot).expect("snapshot json");
    assert!(!encoded.contains("fixture-secret"));
    assert!(encoded.contains(PROVIDER_DIAGNOSTICS_SNAPSHOT_SCHEMA));
    assert!(encoded.contains(PROVIDER_DIAGNOSTICS_CURSOR_SCHEMA));
}

#[test]
fn authority_or_config_epoch_change_discards_projection() {
    let snapshot = snapshot();
    assert_eq!(
        snapshot.validate_for_epoch(8, 11).unwrap_err(),
        PROVIDER_DIAGNOSTICS_PROJECTION_STALE
    );
    assert_eq!(
        snapshot.validate_for_epoch(7, 12).unwrap_err(),
        PROVIDER_DIAGNOSTICS_PROJECTION_STALE
    );
    snapshot
        .validate_for_epoch(7, 11)
        .expect("current projection");
}

#[test]
fn settings_view_cannot_create_connection_test_admission() {
    let mut request = ProviderConnectionTestRequest {
        schema: PROVIDER_CONNECTION_TEST_SCHEMA.to_owned(),
        version: PROVIDER_DIAGNOSTICS_VERSION,
        profile: "default".to_owned(),
        model_id: "model-fixture".to_owned(),
        catalog_digest: digest('c'),
        config_digest: digest('g'),
        gateway_admission_digest: digest('a'),
        explicit: false,
        allow_inference: false,
    };
    assert_eq!(
        request.validate().unwrap_err(),
        PROVIDER_DIAGNOSTICS_CONNECTION_TEST_EXPLICIT
    );
    request.explicit = true;
    request.allow_inference = true;
    request.validate().expect("explicit gateway admission");
}

#[test]
fn terminal_receipt_replays_without_another_model_request() {
    let mut snapshot = snapshot();
    snapshot.terminal = Some(ProviderTerminalReplay {
        schema: PROVIDER_TERMINAL_REPLAY_SCHEMA.to_owned(),
        version: PROVIDER_DIAGNOSTICS_VERSION,
        sequence: 1,
        event_digest: digest('e'),
        receipt_digest: digest('r'),
        outcome: "completed".to_owned(),
    });
    snapshot.snapshot_digest = snapshot.digest();
    snapshot.validate().expect("terminal snapshot");
    assert!(snapshot.terminal_after(0).expect("replay").is_some());
    assert!(snapshot
        .terminal_after(1)
        .expect("already observed")
        .is_none());
}

#[test]
fn unknown_usage_and_actionable_error_are_explicit() {
    let mut unknown = entry(
        ProviderDiagnosticStatus::UsageUnknown,
        ProviderUsageDiagnostic::unknown("provider_did_not_report_usage").expect("reason"),
    );
    unknown.error = Some(
        ProviderDiagnosticError::new("provider_usage_unknown", "retry after provider receipt")
            .expect("error"),
    );
    unknown.terminal_event_digest = Some(digest('u'));
    unknown.validate().expect("unknown usage entry");
}
