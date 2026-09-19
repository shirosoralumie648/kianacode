#[test]
fn digest_contract_binds_normalization_version_and_has_no_effect_path() {
    let source = include_str!("../src/digest.rs");
    assert!(source.contains("normalization_version"));
    assert!(source.contains("DigestKind::Event"));
    assert!(source.contains("DigestKind::Trace"));
    assert!(source.contains("DigestKind::Artifact"));
    assert!(source.contains("DigestKind::Receipt"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
    assert!(!source.contains("CapabilityBrokerPort"));
}
