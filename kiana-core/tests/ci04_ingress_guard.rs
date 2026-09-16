#[test]
fn daemon_ingress_is_checked_before_core_and_legacy_identity_is_explicit() {
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let migration = include_str!("../../kiana-domain/src/identity_contracts.rs");
    for marker in [
        "pub fn validate_protected_ingress",
        "loopback_authority",
        "ingress_origin_not_loopback",
        "ingress_host_not_loopback",
        "ingress_protected_credentials_required",
        "validate_protected_ingress(&request.metadata)",
        "legacy_local_user_migration",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon ingress marker missing: {marker}"
        );
    }
    for marker in [
        "pub instance_id: Option<String>",
        "pub origin: Option<String>",
        "pub host: Option<String>",
        "pub credential_ref: Option<kiana_domain::SecretRef>",
        "pub identity_mode: Option<String>",
    ] {
        assert!(
            protocol.contains(marker),
            "protocol ingress marker missing: {marker}"
        );
    }
    for marker in [
        "pub struct IdentityMigration",
        "IDENTITY_MIGRATION_SCHEMA",
        "identity_migration_identity_unchanged",
        "deny_unknown_fields",
    ] {
        assert!(
            migration.contains(marker),
            "migration marker missing: {marker}"
        );
    }
}
