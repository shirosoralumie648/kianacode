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
        "EXTENSION_MANAGE_OPERATION",
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
    // EXT-22 typed lifecycle binding: the ControlPlane is the only place that mints the
    // mutation digest, and it does so *after* injecting the server-owned actor/project fields.
    for marker in [
        "\"stage\"",
        "ExtensionLifecycleMutation::from_command_arguments",
        "bind_to_mutation",
        "extension_config_snapshot_required",
        "extension_config_snapshot_digest_required",
        "mutation_digest",
    ] {
        assert!(
            commands.contains(marker),
            "missing EXT-22 ControlPlane binding marker: {marker}"
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
    // The daemon re-derives the mutation and compares it with the declared digest, and it
    // rejects stale trust/dependency snapshots before touching the registry CAS. It never
    // mints a mutation of its own.
    for marker in [
        "ExtensionLifecycleMutation::from_command_arguments",
        "extension_lifecycle_mutation_digest_mismatch",
        "extension_trust_revision_stale",
        "extension_dependency_snapshot_stale",
        "project_trust_revision",
        "dependency_snapshot_digest",
        "latest_lifecycle_action",
    ] {
        assert!(
            daemon.contains(marker),
            "missing EXT-22 daemon binding marker: {marker}"
        );
    }
    assert!(!daemon.contains("ExtensionLifecycleMutation::new("));
    assert!(!domain.contains("raw_secret_value"));
}
