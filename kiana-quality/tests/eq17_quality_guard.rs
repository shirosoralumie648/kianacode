#[test]
fn quality_normalizer_has_no_runtime_or_external_effect_dependency() {
    let manifest = include_str!("../Cargo.toml");
    let source = include_str!("../src/normalize.rs");
    assert!(manifest.contains("kiana-domain.workspace = true"));
    assert!(!manifest.contains("tokio"));
    assert!(!manifest.contains("reqwest"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("std::net"));
}
