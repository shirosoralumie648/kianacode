#[test]
fn extension_lifecycle_uses_control_plane_and_append_only_cas_boundaries() {
    let domain = include_str!("../../kiana-domain/src/extension_lifecycle.rs");
    let commands = include_str!("../src/commands.rs");
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    for marker in [
        "ExtensionLifecycleMutation",
        "ExtensionConfigSnapshot",
        "ExtensionLifecycleAction",
        "extension_enable_approval_and_config_required",
        "extension_lifecycle_dependency_snapshot_required",
    ] {
        assert!(
            domain.contains(marker),
            "missing EXT-22 domain marker: {marker}"
        );
    }
    for marker in [
        "extension.manage",
        "extension_mutation_fields_required",
        "expected_registry_version",
        "idempotency_key",
        "operator_authorized",
    ] {
        assert!(
            commands.contains(marker),
            "missing EXT-22 ControlPlane marker: {marker}"
        );
    }
    for marker in [
        "append_idempotent_expected",
        "extension.lifecycle",
        "extension_registry_version_mismatch",
        "extension_activation_event_failed",
    ] {
        assert!(
            daemon.contains(marker),
            "missing EXT-22 daemon marker: {marker}"
        );
    }
    assert!(!domain.contains("raw_secret_value"));
}
