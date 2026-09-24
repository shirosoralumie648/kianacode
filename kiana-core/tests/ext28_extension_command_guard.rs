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
