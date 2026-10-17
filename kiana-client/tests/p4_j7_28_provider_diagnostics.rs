use kiana_client::{ProviderDiagnosticsClientError, ProviderDiagnosticsClientState};
use kiana_domain::*;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn snapshot() -> ProviderDiagnosticsSnapshot {
    let route = ModelRoute {
        provider_id: "provider-fixture".to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: "connection-fixture".to_owned(),
        model_id: "model-fixture".to_owned(),
        profile: "default".to_owned(),
        configuration_revision: "config-rev-1".to_owned(),
        streaming: true,
    };
    let capabilities = ModelCapabilities {
        tools: CapabilitySupport::Supported,
        streaming: CapabilitySupport::Supported,
        structured_output: CapabilitySupport::Unknown,
        images: CapabilitySupport::Unsupported,
        reasoning_replay: CapabilitySupport::Unknown,
        context_window: 16_000,
        max_output: 2_000,
        source: "fixture".to_owned(),
        revision: "cap-rev-1".to_owned(),
    };
    let configuration = ProviderConfigSnapshot::new(
        ProviderSelectionMode::Cassette,
        vec![ProviderProfileSnapshot::new(
            route,
            capabilities.clone(),
            Some(digest('k')),
            ProviderConfigSource::Explicit,
        )
        .expect("profile")],
    )
    .expect("configuration");
    let catalog = ModelCatalog::new(vec![ModelCatalogEntry {
        schema: MODEL_CATALOG_ENTRY_SCHEMA.to_owned(),
        provider_id: "provider-fixture".to_owned(),
        connection_id: "connection-fixture".to_owned(),
        model_id: "model-fixture".to_owned(),
        capabilities: capabilities.clone(),
        source: ModelCatalogSource::Configured,
        catalog_revision: "catalog-rev-1".to_owned(),
        expires_at_unix_ms: None,
    }])
    .expect("catalog");
    let check = ProviderConfigCheck::new(
        ProviderConfigCheckState::StaticValidated,
        configuration.snapshot_digest.clone(),
        1,
        None,
    )
    .expect("config check");
    let entry = ProviderDiagnosticEntry::new(
        "provider-fixture",
        "connection-fixture",
        "default",
        "model-fixture",
        ModelProtocol::OpenAiChat,
        capabilities,
        ProviderDisplayMode::Buffered,
        ProviderConfigCheckState::StaticValidated,
        ProviderDiagnosticStatus::Ready,
        ProviderUsageDiagnostic::Known {
            usage: ModelUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        },
    )
    .expect("diagnostic entry");
    ProviderDiagnosticsSnapshot::new(7, 11, configuration, catalog, check, vec![entry], None)
        .expect("snapshot")
}

fn explicit_connection_test() -> ProviderConnectionTestRequest {
    ProviderConnectionTestRequest {
        schema: PROVIDER_CONNECTION_TEST_SCHEMA.to_owned(),
        version: PROVIDER_DIAGNOSTICS_VERSION,
        profile: "default".to_owned(),
        model_id: "model-fixture".to_owned(),
        catalog_digest: snapshot().catalog.catalog_digest,
        config_digest: snapshot().configuration.snapshot_digest,
        gateway_admission_digest: digest('a'),
        explicit: true,
        allow_inference: true,
        authority_epoch: 7,
        config_epoch: 11,
    }
}

#[test]
fn reconnect_rejects_a_future_sequence_gap_and_clears_old_projection() {
    let current = snapshot();
    let mut client = ProviderDiagnosticsClientState::default();
    client.hydrate(current, 7, 11).expect("hydrate");

    let mut future = snapshot();
    future.cursor = ProviderDiagnosticsCursor::new(7, 11, 3).expect("future cursor");
    future.snapshot_digest = future.digest();
    future.validate().expect("future snapshot");

    assert_eq!(
        client.reconnect(future, 7, 11, Some(2)).unwrap_err(),
        ProviderDiagnosticsClientError::CursorGap
    );
    assert!(client.snapshot().is_none());
    assert!(client.cursor().is_none());
}

#[test]
fn web_reconnect_never_retries_model_request() {
    let current = snapshot();
    let mut client = ProviderDiagnosticsClientState::default();
    client.hydrate(current.clone(), 7, 11).expect("hydrate");
    client.begin_model_attempt().expect("first attempt");
    assert_eq!(client.request_attempts(), 1);

    client
        .reconnect(current.clone(), 7, 11, Some(1))
        .expect("reconnect");
    client
        .reconnect(current, 7, 11, Some(1))
        .expect("repeated reconnect");

    assert_eq!(client.request_attempts(), 1);
    assert_eq!(
        client.begin_model_attempt().unwrap_err(),
        ProviderDiagnosticsClientError::Invalid(
            "provider_model_attempt_already_started".to_owned()
        )
    );

    let mut display = client
        .stream_display(ProviderDisplayMode::Buffered)
        .expect("display");
    display.observe(1, None).expect("frame");
    assert_eq!(display.request_attempts(), 1);
}

#[test]
fn settings_view_is_read_only_until_an_explicit_connection_test() {
    let current = snapshot();
    let mut client = ProviderDiagnosticsClientState::default();
    client.hydrate(current, 7, 11).expect("hydrate");

    let view = client.settings_view().expect("settings view");
    assert!(view.connection_test.is_none());
    assert!(!view.inference_triggered());
    assert_eq!(client.request_attempts(), 0);

    let request = explicit_connection_test();
    let plan = client
        .explicit_connection_test(&request, 7, 11)
        .expect("explicit plan");
    assert_eq!(plan.gateway_admission_digest, digest('a'));
    assert_eq!(
        client
            .explicit_connection_test(&request, 8, 11)
            .unwrap_err(),
        ProviderDiagnosticsClientError::StaleEpoch
    );

    assert!(client
        .admit_upstream_effect(ProviderUpstreamEffect::CatalogRead, false, None)
        .expect("offline catalog")
        .is_none());
    assert_eq!(
        client
            .admit_upstream_effect(ProviderUpstreamEffect::ConnectionTest, true, None)
            .unwrap_err(),
        ProviderDiagnosticsClientError::AdmissionRequired
    );
    assert!(client
        .admit_upstream_effect(ProviderUpstreamEffect::ConnectionTest, true, Some(&request))
        .expect("admitted")
        .is_some());
    assert_eq!(client.request_attempts(), 0);
}

#[test]
fn stale_provider_ui_action_cannot_mutate_active_run() {
    let current = snapshot();
    let mut client = ProviderDiagnosticsClientState::default();
    client.hydrate(current, 7, 11).expect("hydrate");

    let action = ProviderUiAction::new(
        ProviderUiActionKind::Cancel,
        Some("run-fixture".to_owned()),
        7,
        11,
        1,
    )
    .expect("cancel action");
    let disposition = client
        .authorize_ui_action(&action, Some("run-fixture"), 7, 11, 1)
        .expect("current action");
    assert!(disposition.mutates_run);

    assert_eq!(
        client
            .authorize_ui_action(&action, Some("run-fixture"), 8, 11, 1)
            .unwrap_err(),
        ProviderDiagnosticsClientError::StaleAction
    );
    assert_eq!(
        client
            .authorize_ui_action(&action, Some("run-fixture"), 7, 11, 2)
            .unwrap_err(),
        ProviderDiagnosticsClientError::StaleAction
    );
}

#[test]
fn late_subscriber_observes_terminal_from_receipt() {
    let mut current = snapshot();
    current.cursor = ProviderDiagnosticsCursor::new(7, 11, 2).expect("cursor");
    current.terminal = Some(ProviderTerminalReplay {
        schema: PROVIDER_TERMINAL_REPLAY_SCHEMA.to_owned(),
        version: PROVIDER_DIAGNOSTICS_VERSION,
        sequence: 2,
        event_digest: digest('e'),
        receipt_digest: digest('r'),
        outcome: "completed".to_owned(),
    });
    current.snapshot_digest = current.digest();
    current.validate().expect("terminal snapshot");

    let mut client = ProviderDiagnosticsClientState::default();
    client.hydrate(current, 7, 11).expect("hydrate");
    let terminal = client.terminal_after(1).expect("replay").expect("terminal");
    assert_eq!(terminal.receipt_digest, digest('r'));
    assert!(client.terminal_after(2).expect("observed").is_none());
    assert_eq!(client.request_attempts(), 0);
}
