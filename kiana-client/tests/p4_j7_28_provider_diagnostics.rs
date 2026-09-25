use kiana_client::{ProviderDiagnosticsClientError, ProviderDiagnosticsClientState};
use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
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
