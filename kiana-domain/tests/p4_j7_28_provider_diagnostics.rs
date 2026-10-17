use kiana_domain::*;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
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

fn explicit_connection_test() -> ProviderConnectionTestRequest {
    ProviderConnectionTestRequest {
        schema: PROVIDER_CONNECTION_TEST_SCHEMA.to_owned(),
        version: PROVIDER_DIAGNOSTICS_VERSION,
        profile: "default".to_owned(),
        model_id: "model-fixture".to_owned(),
        catalog_digest: catalog().catalog_digest,
        config_digest: configuration().snapshot_digest,
        gateway_admission_digest: digest('a'),
        explicit: true,
        allow_inference: true,
        authority_epoch: 7,
        config_epoch: 11,
    }
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
fn diagnostics_cursor_rejects_zero_sequence() {
    assert!(ProviderDiagnosticsCursor::new(1, 1, 0).is_err());
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
        authority_epoch: 7,
        config_epoch: 11,
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
fn model_settings_view_does_not_trigger_inference() {
    let snapshot = snapshot();
    let view = ProviderSettingsView::read_only(snapshot.clone()).expect("read-only view");
    assert!(!view.inference_triggered());
    assert!(view.connection_test.is_none());
    view.validate().expect("read-only settings view");

    let mut unauthorised = explicit_connection_test();
    unauthorised.explicit = false;
    unauthorised.allow_inference = false;
    assert!(
        ProviderSettingsView::with_connection_test(snapshot.clone(), &unauthorised, 7, 11).is_err()
    );

    let request = explicit_connection_test();
    let plan = snapshot
        .connection_test_plan(&request, 7, 11)
        .expect("explicit plan");
    assert_eq!(plan.authority_epoch, 7);
    assert_eq!(plan.config_epoch, 11);
    assert_eq!(
        snapshot.connection_test_plan(&request, 8, 11).unwrap_err(),
        PROVIDER_DIAGNOSTICS_PROJECTION_STALE
    );

    let explicit_view =
        ProviderSettingsView::with_connection_test(snapshot, &request, 7, 11).expect("explicit");
    assert!(explicit_view.connection_test.is_some());
    assert!(!explicit_view.inference_triggered());
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

#[test]
fn three_surfaces_show_same_provider_diagnostics() {
    let snapshot = snapshot();
    let surfaces = [
        ProviderSurface::Cli,
        ProviderSurface::Workbench,
        ProviderSurface::Web,
        ProviderSurface::Desktop,
    ];
    let projections = surfaces
        .into_iter()
        .map(|surface| {
            ProviderSurfaceProjection::new(surface, snapshot.clone()).expect("surface projection")
        })
        .collect::<Vec<_>>();

    let expected = projections[0].shared_digest();
    for projection in &projections[1..] {
        assert_eq!(projection.shared_digest(), expected);
        assert_eq!(projection.snapshot, projections[0].snapshot);
        assert_eq!(
            projection.actionable_errors,
            projections[0].actionable_errors
        );
    }
    assert_eq!(projections[0].surface, ProviderSurface::Cli);
    assert_eq!(projections[1].surface, ProviderSurface::Workbench);
    assert_eq!(projections[2].surface, ProviderSurface::Web);
    assert_eq!(projections[3].surface, ProviderSurface::Desktop);
    assert!(ProviderSurface::Web.shares_web_projection());
    assert!(ProviderSurface::Desktop.shares_web_projection());
    assert!(!ProviderSurface::Cli.shares_web_projection());
    for projection in &projections {
        projection.validate().expect("valid surface projection");
        let encoded = serde_json::to_string(projection).expect("surface json");
        for forbidden in ["raw_body", "authorization", "bearer ", "api_key"] {
            assert!(
                !encoded.contains(forbidden),
                "surface projection leaked protocol detail: {forbidden}"
            );
        }
    }
}

#[test]
fn stale_provider_ui_action_cannot_mutate_active_run() {
    let action = ProviderUiAction::new(
        ProviderUiActionKind::Cancel,
        Some("run-fixture".to_owned()),
        7,
        11,
        4,
    )
    .expect("cancel action");
    let disposition = action
        .authorize(Some("run-fixture"), 7, 11, 4)
        .expect("current action");
    assert!(disposition.mutates_run);

    assert_eq!(
        action.authorize(Some("run-fixture"), 8, 11, 4).unwrap_err(),
        PROVIDER_UI_ACTION_STALE
    );
    assert_eq!(
        action.authorize(Some("run-fixture"), 7, 11, 5).unwrap_err(),
        PROVIDER_UI_ACTION_STALE
    );
    assert_eq!(
        action.authorize(Some("run-other"), 7, 11, 4).unwrap_err(),
        PROVIDER_UI_ACTION_RUN_MISMATCH
    );

    let refresh = ProviderUiAction::new(ProviderUiActionKind::RetryDisplay, None, 7, 11, 4)
        .expect("display retry");
    let disposition = refresh.authorize(None, 7, 11, 4).expect("read-only retry");
    assert!(!disposition.mutates_run);

    assert!(ProviderUiAction::new(ProviderUiActionKind::Cancel, None, 7, 11, 4).is_err());
}

#[test]
fn late_subscriber_observes_terminal_from_receipt() {
    let mut snapshot = snapshot();
    snapshot.cursor = ProviderDiagnosticsCursor::new(7, 11, 4).expect("observed sequence");
    let terminal = ProviderTerminalReplay {
        schema: PROVIDER_TERMINAL_REPLAY_SCHEMA.to_owned(),
        version: PROVIDER_DIAGNOSTICS_VERSION,
        sequence: 4,
        event_digest: digest('e'),
        receipt_digest: digest('r'),
        outcome: "completed".to_owned(),
    };
    terminal.validate().expect("terminal replay");
    snapshot.terminal = Some(terminal);
    snapshot.snapshot_digest = snapshot.digest();
    snapshot.validate().expect("terminal snapshot");

    let replay = snapshot
        .terminal_after(2)
        .expect("late subscriber replay")
        .cloned()
        .expect("terminal exists");
    assert_eq!(replay.sequence, 4);
    assert_eq!(replay.receipt_digest, digest('r'));
    assert!(snapshot.terminal_after(4).expect("observed").is_none());
}

#[test]
fn no_stream_collects_without_second_model_request() {
    let terminal = ProviderTerminalReplay {
        schema: PROVIDER_TERMINAL_REPLAY_SCHEMA.to_owned(),
        version: PROVIDER_DIAGNOSTICS_VERSION,
        sequence: 3,
        event_digest: digest('e'),
        receipt_digest: digest('r'),
        outcome: "completed".to_owned(),
    };
    let mut display =
        ProviderStreamDisplay::new(ProviderDisplayMode::Buffered, 7, 11, 1).expect("display");

    display.observe(1, None).expect("first frame");
    display.observe(2, None).expect("second frame");
    display.observe(3, Some(terminal)).expect("terminal frame");

    assert_eq!(display.request_attempts(), 1);
    assert!(display.closed);
    assert!(display.terminal.is_some());
    assert_eq!(
        display.observe(4, None).unwrap_err(),
        "provider_stream_display_closed"
    );

    let mut gapped =
        ProviderStreamDisplay::new(ProviderDisplayMode::Native, 7, 11, 1).expect("display");
    assert_eq!(
        gapped.observe(2, None).unwrap_err(),
        PROVIDER_STREAM_DISPLAY_SEQUENCE_GAP
    );
    assert!(ProviderStreamDisplay::new(ProviderDisplayMode::Synthetic, 7, 11, 2).is_err());
}

#[test]
fn live_provider_effects_require_explicit_gateway_admission() {
    assert!(admit_provider_upstream_effect(
        ProviderUpstreamEffect::CatalogRead,
        false,
        None,
        7,
        11
    )
    .expect("offline catalog")
    .is_none());
    assert!(
        admit_provider_upstream_effect(ProviderUpstreamEffect::ModelSmoke, false, None, 7, 11)
            .expect("fake smoke")
            .is_none()
    );
    assert_eq!(
        admit_provider_upstream_effect(ProviderUpstreamEffect::ConnectionTest, true, None, 7, 11)
            .unwrap_err(),
        PROVIDER_LIVE_EFFECT_ADMISSION_REQUIRED
    );

    let request = explicit_connection_test();
    for effect in [
        ProviderUpstreamEffect::CatalogRead,
        ProviderUpstreamEffect::ModelSmoke,
        ProviderUpstreamEffect::ConnectionTest,
    ] {
        let admission = admit_provider_upstream_effect(effect, true, Some(&request), 7, 11)
            .expect("admitted effect")
            .expect("live admission");
        assert_eq!(admission.effect, effect);
        admission.validate().expect("valid admission");
    }
    assert_eq!(
        admit_provider_upstream_effect(
            ProviderUpstreamEffect::ConnectionTest,
            true,
            Some(&request),
            8,
            11
        )
        .unwrap_err(),
        PROVIDER_DIAGNOSTICS_PROJECTION_STALE
    );
}

#[test]
fn save_static_validate_and_live_verify_are_distinct_states() {
    let configuration = configuration();
    let saved =
        ProviderConfigCheck::saved(configuration.snapshot_digest.as_str(), 1).expect("saved");
    let static_check =
        ProviderConfigCheck::static_validated(configuration.snapshot_digest.as_str(), 2)
            .expect("static");
    assert_eq!(saved.state, ProviderConfigCheckState::Saved);
    assert!(!saved.state.is_live());
    assert_eq!(
        static_check.state,
        ProviderConfigCheckState::StaticValidated
    );
    assert!(!static_check.state.is_live());

    let live = ProviderConfigCheck::live_verified(
        configuration.snapshot_digest.as_str(),
        3,
        Some(digest('v')),
    )
    .expect("live");
    assert!(live.state.is_live());
    assert!(
        ProviderConfigCheck::live_verified(configuration.snapshot_digest.as_str(), 3, None)
            .is_err()
    );
}
