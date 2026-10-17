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
        "snapshot.validate()",
        "serde_json::to_value(snapshot)",
        "visibility_json",
        "metadata-only",
    ] {
        assert!(
            projection.contains(marker),
            "missing projection marker: {marker}"
        );
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
        assert!(
            domain.contains(marker),
            "missing generation fence marker: {marker}"
        );
    }
    assert!(projection.contains("visibility_action_intent"));
    assert!(projection.contains("ControlPlane") || projection.contains("DaemonHost"));
    assert!(daemon.contains("action labels are intents"));
    assert!(daemon.contains("extension.manage"));
    assert!(fixture.contains("visibility_projection_omits_body_paths_and_secrets"));
    assert!(fixture.contains("untrusted_visibility_entry_is_rejected"));
}

#[test]
fn search_is_read_only_and_the_runtime_fixtures_stay_wired_to_the_lane() {
    let core = include_str!("../../kiana-core/src/commands.rs");
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let catalog = include_str!("../../kiana-domain/src/operation_catalog.rs");
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    let entrypoints = include_str!("../../kiana-entrypoints/tests/ext27_extension_visibility.rs");
    let lane = include_str!("../../.github/workflows/ext27-extension-visibility.yml");

    // `search` reads the same trust-filtered projection as `list`. Classifying it as a mutation
    // made the bounded query demand lifecycle-binding fields a read-only caller cannot have.
    assert!(core.contains("\"list\" | \"inspect\" | \"search\""));
    assert!(core.contains("extension_visibility_query_invalid"));
    assert!(core.contains("extension_mutation_fields_required"));
    // The domain risk floor must agree with the command classification: a ReadOnly `search` would
    // otherwise fail `capability_action_contract` with `action_risk_downgrade` before policy runs.
    assert!(catalog.contains("risk_rule: DynamicRiskRule::ExtensionVisibilityAction"));
    assert!(actions.contains("DynamicRiskRule::ExtensionVisibilityAction"));
    assert!(actions.contains("\"list\" | \"search\" | \"inspect\""));

    for fixture in [
        "ext27_visibility_projection_is_redacted_bounded_and_role_scoped",
        "ext27_visibility_projection_identity_fences_stale_surface_caches",
    ] {
        assert!(
            daemon.contains(fixture),
            "missing daemon fixture: {fixture}"
        );
    }
    for fixture in [
        "unknown_trust_project_has_no_projection_and_the_refusal_leaks_nothing",
        "four_surfaces_read_one_redacted_snapshot_and_stale_caches_are_fenced",
    ] {
        assert!(
            entrypoints.contains(fixture),
            "missing surface fixture: {fixture}"
        );
    }
    for surface in [
        "EntryPointKind::Cli",
        "EntryPointKind::Workbench",
        "EntryPointKind::Web",
        "EntryPointKind::Desktop",
    ] {
        assert!(entrypoints.contains(surface), "missing surface: {surface}");
    }
    for deny in [
        "project_untrusted",
        "extension_visibility_snapshot_stale",
        "extension_visibility_action_denied",
        "extension_visibility_not_found",
    ] {
        assert!(entrypoints.contains(deny), "missing deny marker: {deny}");
    }

    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("cargo fmt --all --check"));
    assert!(lane.contains("cargo test -p kiana-domain --test ext27_visibility"));
    assert!(lane.contains("cargo test -p kiana-core --test ext27_visibility_guard"));
    assert!(lane.contains("cargo test -p kiana-daemon --lib ext27_visibility"));
    assert!(lane.contains("cargo test -p kiana-entrypoints --test ext27_extension_visibility"));
}
