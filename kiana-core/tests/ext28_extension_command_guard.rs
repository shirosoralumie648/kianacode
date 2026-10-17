#[test]
fn extension_commands_use_typed_contract_and_shared_control_plane_route() {
    let domain = include_str!("../../kiana-domain/src/extension_commands.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let core = include_str!("../src/commands.rs");
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    let projection = include_str!("../../kiana-entrypoints/src/extension_projection.rs");

    for command in [
        "extension.list",
        "extension.inspect",
        "extension.install",
        "extension.enable",
        "extension.disable",
        "extension.revoke",
        "extension.rollback",
    ] {
        assert!(domain.contains(command), "missing typed command {command}");
    }
    for marker in [
        "ExtensionCommandRequest",
        "ExtensionCommandReceipt",
        "ExtensionCommandError",
        "extension_command(",
    ] {
        assert!(
            protocol.contains(marker),
            "missing protocol marker {marker}"
        );
    }
    assert!(client.contains("pub async fn extension_command"));
    assert!(projection.contains("extension_command_on_host"));
    assert!(core.contains("ExtensionCommand::from_wire_name"));
    assert!(core.contains("extension_operator_required"));
    assert!(daemon.contains("\"enable\" | \"disable\""));
    assert!(daemon.contains("append_idempotent_expected"));
    assert!(daemon.contains("command_receipt"));
    assert!(daemon.contains("disabled"));
    assert!(daemon.contains("request.request.risk"));
}

#[test]
fn ui_adapter_does_not_execute_packages_or_hooks() {
    let projection = include_str!("../../kiana-entrypoints/src/extension_projection.rs");
    assert!(!projection.contains("load_source"));
    assert!(!projection.contains("execute_hook"));
    assert!(!projection.contains("ExtensionRegistry"));
    assert!(projection.contains("DaemonHost"));
}

#[test]
fn command_binding_and_receipt_projection_stay_on_the_shared_route() {
    let domain = include_str!("../../kiana-domain/src/extension_commands.rs");
    let core = include_str!("../src/commands.rs");
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    let projection = include_str!("../../kiana-entrypoints/src/extension_projection.rs");

    // The typed DTO and the shared `extension.manage` route must agree on the lifecycle binding
    // keys; otherwise a typed install/enable could never pass ControlPlane normalization.
    for key in [
        "trust_revision",
        "dependency_snapshot_digest",
        "config_snapshot_digest",
        "config_snapshot",
        "approval_id",
    ] {
        assert!(domain.contains(key), "domain binding key missing: {key}");
        assert!(core.contains(key), "route whitelist missing: {key}");
    }
    for code in [
        "extension_command_lifecycle_binding_required",
        "extension_command_approval_required",
        "extension_command_config_snapshot_required",
        "extension_command_read_only_mutation_fields",
        "extension_command_mutation_fields_required",
    ] {
        assert!(domain.contains(code), "missing refusal code: {code}");
    }
    // The receipt projection decodes and re-validates what the daemon committed; it never
    // re-derives a receipt from local state.
    assert!(domain.contains("pub fn from_response_output"));
    assert!(domain.contains("extension_command_receipt_missing"));
    assert!(projection.contains("pub fn command_receipt"));
    assert!(projection.contains("pub fn command_receipt_json"));
    // The replay path answers with the same typed receipt shape as the original commit.
    assert!(daemon.contains("EXTENSION_COMMAND_RECEIPT_SCHEMA"));
    assert!(daemon.contains("must answer with the same typed receipt shape"));
}

#[test]
fn ext28_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    let entrypoints = include_str!("../../kiana-entrypoints/tests/ext28_extension_commands.rs");
    let lane = include_str!("../../.github/workflows/ext28-extension-commands.yml");

    for fixture in [
        "ext28_commands_are_permission_bound_and_the_receipt_resolves_to_the_committed_event",
        "ext28_replayed_commands_return_the_same_receipt_and_event_without_a_second_fact",
    ] {
        assert!(
            daemon.contains(fixture),
            "missing daemon fixture: {fixture}"
        );
    }
    for fixture in [
        "untrusted_project_refuses_every_command_before_authority_and_leaks_nothing",
        "trusted_project_keeps_reads_free_and_state_changes_approval_bound",
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
        "extension_operator_required",
        "extension_mutation_fields_required",
        "external_side_effect_requires_approval",
        "extension_command_receipt_missing",
    ] {
        assert!(entrypoints.contains(deny), "missing deny marker: {deny}");
    }

    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("cargo fmt --all --check"));
    assert!(lane.contains("cargo test -p kiana-domain --test ext28_extension_commands"));
    assert!(lane.contains("cargo test -p kiana-protocol --test ext28_extension_command_wire"));
    assert!(lane.contains("cargo test -p kiana-core --test ext28_extension_command_guard"));
    assert!(lane.contains("cargo test -p kiana-daemon --lib ext28_"));
    assert!(lane.contains("cargo test -p kiana-entrypoints --test ext28_extension_commands"));
}
