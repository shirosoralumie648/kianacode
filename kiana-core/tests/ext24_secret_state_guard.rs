#[test]
fn extension_secret_state_and_migration_boundaries_are_wired_fail_closed() {
    let domain = include_str!("../../kiana-domain/src/extension_state.rs");
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");

    for marker in [
        "pub struct ExtensionSecretBinding",
        "pub struct ExtensionConfigurationSnapshot",
        "pub struct ExtensionStateScope",
        "pub struct ExtensionStateMigrationPlan",
        "pub struct ExtensionStateMigrationReceipt",
        "validate_at",
        "extension_configuration_raw_secret_forbidden",
        "ExtensionStateMigrationStatus",
        "RetainedOld",
        "Unknown",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "missing EXT-24 domain marker: {marker}"
        );
    }
    for marker in [
        "state_root",
        "cache_root",
        "extension_home_inside_project",
        "parse_configuration_snapshot",
        "read_migration_declaration",
        "prepare_controlled_migration",
        "extension_state_migration_requires_controlled_action",
        "ExtensionStateMigrationReceipt::unknown",
    ] {
        assert!(
            daemon.contains(marker),
            "missing EXT-24 daemon marker: {marker}"
        );
    }
    for source in [domain, daemon] {
        for forbidden in [
            "pub secret_value",
            "pub secret: String",
            "credential_passthrough",
            "token_passthrough",
        ] {
            assert!(
                !source.contains(forbidden),
                "raw secret/passthrough marker must stay absent: {forbidden}"
            );
        }
    }
}
