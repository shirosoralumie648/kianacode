#[test]
fn ports_keep_identity_config_credential_and_rotation_boundaries_separate() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/identity_contracts.rs");
    for marker in [
        "pub trait IdentityResolver",
        "pub trait CredentialResolver",
        "pub trait ConfigSnapshotStore",
        "pub trait CredentialRotationPort",
        "pub use CredentialRotationPort as RotationRevokePort",
        "CredentialResolution",
        "Serialize, Deserialize",
        "deny_unknown_fields",
        "rename_all = \"snake_case\"",
        "credential_resolution_expired",
        "credential_resolution_digest_invalid",
        "raw secret bytes or strings",
    ] {
        assert!(ports.contains(marker), "port marker missing: {marker}");
    }
    for marker in [
        "pub struct SecretRef",
        "pub struct ConfigSnapshot",
        "pub struct AuthoritySnapshot",
        "reference_digest",
        "snapshot_digest",
    ] {
        assert!(
            domain.contains(marker),
            "domain contract marker missing: {marker}"
        );
    }
}
