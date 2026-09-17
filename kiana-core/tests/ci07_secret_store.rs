#[test]
fn credential_effect_boundary_keeps_secret_resolution_out_of_core_and_event_shapes() {
    let domain = include_str!("../../kiana-domain/src/credentials.rs");
    let config = include_str!("../../kiana-provider/src/config.rs");
    let provider = include_str!("../../kiana-provider/src/credentials.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    for marker in [
        "CredentialLease",
        "CREDENTIAL_LEASE_DEFAULT_TTL_MS",
        "validate_for",
        "credential_lease_replayed",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "domain lease marker missing: {marker}"
        );
    }
    for marker in [
        "credential_ref: Option<SecretRef>",
        "credential_store: std::sync::Arc<dyn SecretStore>",
        "credential_revision",
        "SecretRef::new",
    ] {
        assert!(
            config.contains(marker),
            "connection marker missing: {marker}"
        );
    }
    assert!(!config.contains("credential: Option<String>"));
    for marker in [
        "trait SecretStore",
        "EnvSecretStore",
        "InlineSecretStore",
        "KeyringSecretStore",
        "FileSecretStore",
        "OsSecretStore",
        "credential_backend_unsupported",
        "impl Drop for SecretMaterial",
    ] {
        assert!(
            provider.contains(marker),
            "provider store marker missing: {marker}"
        );
    }
    for marker in [
        "credential_store.issue",
        "validate_for",
        ".lease.consume",
        "connection.endpoint",
    ] {
        assert!(
            transport.contains(marker),
            "transport lease marker missing: {marker}"
        );
    }
    for marker in [
        "consume_credential_lease",
        "effect",
        "CredentialLease",
        "credential_lease_invalid",
    ] {
        assert!(
            broker.contains(marker),
            "broker lease marker missing: {marker}"
        );
    }
    assert!(ports.contains("CredentialResolver"));
    assert!(ports.contains("never returns raw secret bytes or strings"));
    assert!(!domain.contains("pub secret_value"));
    assert!(!domain.contains("pub secret: String"));
}
