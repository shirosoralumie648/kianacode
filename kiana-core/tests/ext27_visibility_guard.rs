#[test]
fn all_product_surfaces_render_one_server_owned_visibility_projection() {
    let projection = include_str!("../../kiana-entrypoints/src/extension_projection.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let core = include_str!("../../kiana-core/src/commands.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");

    for surface in [cli, web, workbench] {
        assert!(
            surface.contains("extension_projection")
                || surface.contains("extension_visibility_on_host"),
            "surface must use the shared extension projection adapter"
        );
    }
    for marker in [
        "ExtensionVisibilitySnapshot",
        "snapshot_id",
        "generation",
        "visibility_json",
        "metadata-only",
    ] {
        assert!(projection.contains(marker), "missing projection marker: {marker}");
    }
    assert!(web.contains("/api/extensions"));
    assert!(web.contains("EntryPointKind::Desktop"));
    assert!(workbench.contains("EntryPointKind::Workbench"));
    assert!(client.contains("EXTENSION_MANAGE_OPERATION"));
    assert!(daemon.contains("extension_visibility_snapshot"));
    assert!(daemon.contains("ControlPlane") || daemon.contains("extension.manage"));
    assert!(core.contains("authorize_and_execute"));
    assert!(broker.contains("CapabilityBroker"));
}

#[test]
fn visibility_actions_remain_intents_and_caches_are_generation_fenced() {
    let domain = include_str!("../../kiana-domain/src/extension_visibility.rs");
    let projection = include_str!("../../kiana-entrypoints/src/extension_projection.rs");
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    let fixture = include_str!("../../kiana-domain/tests/ext27_visibility.rs");

    for marker in [
        "require_generation",
        "extension_visibility_snapshot_stale",
        "snapshot_digest",
        "ExtensionVisibilityAction",
    ] {
        assert!(domain.contains(marker), "missing generation fence marker: {marker}");
    }
    assert!(projection.contains("visibility_action_intent"));
    assert!(projection.contains("ControlPlane") || projection.contains("DaemonHost"));
    assert!(daemon.contains("action labels are intents"));
    assert!(daemon.contains("extension.manage"));
    assert!(fixture.contains("visibility_projection_omits_body_paths_and_secrets"));
    assert!(fixture.contains("untrusted_visibility_entry_is_rejected"));
}

