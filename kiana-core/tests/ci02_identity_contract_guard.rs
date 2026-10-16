//! CI-02 Core 静态 identity contract/export 守卫。
//!
//! `include_str!` 读取 Domain 类型、稳定 ID registry、schema contract 和 protocol exports 并匹配关键文本。
//! 它不构造身份、不连接 authority store，也不能证明 DTO 已被运行时消费。

#[test]
/// 确认 Principal/SecretRef/ConfigSnapshot/AuthoritySnapshot 及稳定 ID/schema 导出标记仍存在。
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
