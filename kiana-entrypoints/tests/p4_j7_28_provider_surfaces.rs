//! P4-J7-28 surface projection fixtures.
//!
//! These fixtures only render server-owned snapshots. They never open a provider connection,
//! create a connection test or issue a model request.

use kiana_domain::{
    CapabilitySupport, ModelCapabilities, ModelCatalog, ModelCatalogEntry, ModelCatalogSource,
    ModelProtocol, ModelRoute, ModelUsage, ProviderConfigCheck, ProviderConfigCheckState,
    ProviderConfigSnapshot, ProviderConfigSource, ProviderDiagnosticEntry,
    ProviderDiagnosticStatus, ProviderDiagnosticsCursor, ProviderDiagnosticsSnapshot,
    ProviderDisplayMode, ProviderProfileSnapshot, ProviderSelectionMode, ProviderStreamDisplay,
    ProviderSurface, ProviderTerminalReplay, ProviderUsageDiagnostic, MODEL_CATALOG_ENTRY_SCHEMA,
    PROVIDER_DIAGNOSTICS_VERSION, PROVIDER_TERMINAL_REPLAY_SCHEMA,
};
use kiana_entrypoints::provider_diagnostics::{
    render_provider_config_check, render_provider_settings, render_provider_surface,
};
use serde_json::Value;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
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

fn snapshot() -> ProviderDiagnosticsSnapshot {
    let configuration = ProviderConfigSnapshot::new(
        ProviderSelectionMode::Cassette,
        vec![ProviderProfileSnapshot::new(
            route(),
            capabilities(),
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
        capabilities: capabilities(),
        source: ModelCatalogSource::Configured,
        catalog_revision: "catalog-rev-1".to_owned(),
        expires_at_unix_ms: None,
    }])
    .expect("catalog");
    let check = ProviderConfigCheck::static_validated(configuration.snapshot_digest.as_str(), 1)
        .expect("config check");
    let entry = ProviderDiagnosticEntry::new(
        "provider-fixture",
        "connection-fixture",
        "default",
        "model-fixture",
        ModelProtocol::OpenAiChat,
        capabilities(),
        ProviderDisplayMode::Buffered,
        ProviderConfigCheckState::StaticValidated,
        ProviderDiagnosticStatus::Ready,
        ProviderUsageDiagnostic::Known {
            usage: ModelUsage {
                input_tokens: 12,
                output_tokens: 8,
            },
        },
    )
    .expect("diagnostic entry");
    ProviderDiagnosticsSnapshot::new(7, 11, configuration, catalog, check, vec![entry], None)
        .expect("snapshot")
}

#[test]
fn three_surfaces_show_same_provider_diagnostics() {
    let snapshot = snapshot();
    let cli = render_provider_surface(&snapshot, ProviderSurface::Cli).expect("cli");
    let workbench =
        render_provider_surface(&snapshot, ProviderSurface::Workbench).expect("workbench");
    let web = render_provider_surface(&snapshot, ProviderSurface::Web).expect("web");
    let desktop = render_provider_surface(&snapshot, ProviderSurface::Desktop).expect("desktop");

    assert_eq!(cli["surface"], "cli");
    assert_eq!(workbench["surface"], "workbench");
    assert_eq!(web["surface"], "web");
    assert_eq!(desktop["surface"], "desktop");
    for rendered in [&cli, &workbench, &web, &desktop] {
        assert_eq!(rendered["snapshot"], cli["snapshot"]);
        assert_eq!(rendered["actionable_errors"], cli["actionable_errors"]);
        assert_eq!(rendered["schema"], "kiana.provider-surface-projection.v1");
        let encoded = rendered.to_string();
        for forbidden in ["raw_body", "authorization", "bearer ", "api_key"] {
            assert!(
                !encoded.contains(forbidden),
                "surface leaked protocol detail: {forbidden}"
            );
        }
    }
    assert_eq!(web["snapshot"], desktop["snapshot"]);
}

#[test]
fn model_settings_view_does_not_trigger_inference() {
    let snapshot = snapshot();
    let view = render_provider_settings(&snapshot).expect("settings view");
    assert_eq!(view["schema"], "kiana.provider-settings-view.v1");
    assert!(view["connection_test"].is_null());
    assert_eq!(
        view["snapshot"]["snapshot_digest"],
        snapshot.snapshot_digest
    );
    let encoded = view.to_string();
    assert!(!encoded.contains("gateway_admission_digest"));
    assert!(!encoded.contains("fixture-secret"));

    let saved = ProviderConfigCheck::saved(snapshot.configuration.snapshot_digest.as_str(), 1)
        .expect("saved");
    let static_check =
        ProviderConfigCheck::static_validated(snapshot.configuration.snapshot_digest.as_str(), 2)
            .expect("static");
    let live = ProviderConfigCheck::live_verified(
        snapshot.configuration.snapshot_digest.as_str(),
        3,
        Some(digest('v')),
    )
    .expect("live");

    for (check, expected) in [
        (saved, "saved"),
        (static_check, "static_validated"),
        (live, "live_verified"),
    ] {
        let rendered = render_provider_config_check(&check).expect("config check");
        assert_eq!(rendered["state"], expected);
        assert_eq!(
            rendered["snapshot_digest"],
            snapshot.configuration.snapshot_digest
        );
        assert!(!rendered.to_string().contains("fixture-secret"));
    }
    assert!(ProviderConfigCheck::live_verified(
        snapshot.configuration.snapshot_digest.as_str(),
        3,
        None
    )
    .is_err());
}

#[test]
fn late_terminal_replay_and_buffered_collection_use_one_request() {
    let mut snapshot = snapshot();
    snapshot.cursor = ProviderDiagnosticsCursor::new(7, 11, 2).expect("cursor");
    snapshot.terminal = Some(ProviderTerminalReplay {
        schema: PROVIDER_TERMINAL_REPLAY_SCHEMA.to_owned(),
        version: PROVIDER_DIAGNOSTICS_VERSION,
        sequence: 2,
        event_digest: digest('e'),
        receipt_digest: digest('r'),
        outcome: "completed".to_owned(),
    });
    snapshot.snapshot_digest = snapshot.digest();
    snapshot.validate().expect("terminal snapshot");

    let terminal = snapshot
        .terminal_after(1)
        .expect("replay")
        .cloned()
        .expect("terminal");
    assert_eq!(terminal.receipt_digest, digest('r'));
    assert!(snapshot.terminal_after(2).expect("observed").is_none());

    let mut display =
        ProviderStreamDisplay::new(ProviderDisplayMode::Synthetic, 7, 11, 1).expect("display");
    display.observe(1, None).expect("text frame");
    display.observe(2, Some(terminal)).expect("terminal frame");
    assert_eq!(display.request_attempts(), 1);
    assert!(display.closed);
}

#[test]
fn rendered_surfaces_are_serializable_json_objects() {
    let snapshot = snapshot();
    for surface in [
        ProviderSurface::Cli,
        ProviderSurface::Workbench,
        ProviderSurface::Web,
        ProviderSurface::Desktop,
    ] {
        let rendered = render_provider_surface(&snapshot, surface).expect("surface");
        assert!(matches!(rendered, Value::Object(_)));
    }
}
