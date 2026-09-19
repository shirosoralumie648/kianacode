#[test]
fn sc27_daemon_keeps_lifecycle_cas_sandbox_and_permit_only_boundaries() {
    let extensions = include_str!("../src/extensions.rs");
    let hooks = include_str!("../src/pre_tool_hooks.rs");
    let sandbox = include_str!("../src/harness_sandbox.rs");
    for marker in [
        "append_idempotent_expected",
        "extension_registry_version_mismatch",
        "extension_execution_snapshot_inactive",
        "uninstalled",
        "uninstall",
        "run_confined_cancellable",
        "extension_operation_mismatch",
    ] {
        assert!(
            extensions.contains(marker) || hooks.contains(marker) || sandbox.contains(marker),
            "missing SC-27 daemon marker: {marker}"
        );
    }
    for marker in [
        "sandbox:\"read-only\"",
        "hook_configuration_changed",
        "hook.decision",
        "project_untrusted",
        "cancelled:hook_not_started",
    ] {
        assert!(
            hooks.contains(marker),
            "missing SC-27 hook marker: {marker}"
        );
    }
    assert!(extensions.contains("extension_activation_event_failed"));
    assert!(extensions.contains("ExtensionAdmission"));
    assert!(sandbox.contains("set_no_new_privs"));
    assert!(sandbox.contains("AMBIENT_AUTHORITY_ENV_VARS"));
    assert!(!extensions.contains("std::process::Command"));
    assert!(!hooks.contains("CapabilityBroker"));
}
