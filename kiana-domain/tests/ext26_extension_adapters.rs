use kiana_domain::{
    validate_declarative_package_entry, ExtensionAdapterDescriptor, ExtensionAdapterStatus,
    ExtensionEffect, ExtensionNetworkPolicy, REASON_BINDING_REQUIRED, REASON_ENTRY_MISSING,
    REASON_ENTRY_NOT_DECLARATIVE,
};
use std::collections::BTreeMap;

#[test]
fn declaration_only_constructor_cannot_advertise_an_adapter() {
    let descriptor = ExtensionAdapterDescriptor::new(
        "demo",
        "main",
        kiana_domain::ExtensionComponentKind::Skill,
        "SKILL.md",
        true,
        true,
        ExtensionEffect::ReadOnly,
        &ExtensionNetworkPolicy::Deny,
    )
    .expect("metadata descriptor");
    assert_eq!(descriptor.status, ExtensionAdapterStatus::Denied);
    assert_eq!(descriptor.reason, REASON_BINDING_REQUIRED);
    descriptor.validate().expect("stable denied descriptor");
}

#[test]
fn package_entry_validation_is_read_only_and_deny_first() {
    let mut files = BTreeMap::new();
    files.insert("SKILL.md".to_owned(), "2320".to_owned());
    let digest = validate_declarative_package_entry(&files, "SKILL.md").unwrap();
    assert!(digest.starts_with("sha256:"));
    assert_eq!(
        validate_declarative_package_entry(&files, "run.sh").unwrap_err(),
        REASON_ENTRY_NOT_DECLARATIVE
    );
    assert_eq!(
        validate_declarative_package_entry(&files, "missing.json").unwrap_err(),
        REASON_ENTRY_MISSING
    );
}

#[test]
fn adapter_source_has_no_direct_execution_or_network_boundary() {
    let source = include_str!("../src/extension_adapters.rs");
    for forbidden in [
        "std::process::Command",
        "tokio::process",
        "TcpStream",
        "reqwest",
        "connect(",
        "spawn(",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden adapter primitive: {forbidden}"
        );
    }
    assert!(source.contains("validate_adapter_binding"));
    assert!(source.contains("REASON_NETWORK_UNAVAILABLE"));
    assert!(source.contains("REASON_COMPONENT_UNSUPPORTED"));
}
