#[test]
fn identity_contracts_keep_secret_and_authority_metadata_typed() {
    let domain = include_str!("../../kiana-domain/src/identity_contracts.rs");
    let ids = include_str!("../../kiana-domain/src/ids.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "pub struct Principal",
        "pub struct Membership",
        "pub struct SecretRef",
        "pub struct ProviderAccount",
        "pub struct ServiceIdentity",
        "pub struct ConfigSnapshot",
        "pub struct AuthoritySnapshot",
        "deny_unknown_fields",
        "authority_epoch",
        "contains_raw_secret",
        "validate_current_epoch",
    ] {
        assert!(
            domain.contains(marker),
            "identity contract marker missing: {marker}"
        );
    }
    for marker in ["PrincipalId", "ProviderAccountId", "ServiceIdentityId"] {
        assert!(
            ids.contains(marker),
            "stable identity ID marker missing: {marker}"
        );
        assert!(contracts.contains(&format!("type_name: \"{marker}\"")));
    }
    for marker in [
        "AuthoritySnapshot",
        "ConfigSnapshot",
        "ProviderAccount",
        "SecretRef",
        "ServiceIdentity",
        "AUTHORITY_SNAPSHOT_SCHEMA",
        "CONFIG_SNAPSHOT_SCHEMA",
    ] {
        assert!(
            protocol.contains(marker),
            "protocol identity export missing: {marker}"
        );
    }
}
