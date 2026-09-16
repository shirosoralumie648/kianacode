#[test]
fn provider_extraction_keeps_one_production_gateway_and_legacy_services_test_only() {
    let provider_manifest = include_str!("../../kiana-provider/Cargo.toml");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/model_client.rs");
    let runner_manifest = include_str!("../../kiana-runner/Cargo.toml");
    let core_manifest = include_str!("../Cargo.toml");

    for forbidden in [
        "kiana-services",
        "kiana-core",
        "kiana-entrypoints",
        "kiana-runner",
    ] {
        assert!(
            !provider_manifest.contains(forbidden),
            "provider has legacy dependency: {forbidden}"
        );
        assert!(
            !provider.contains(forbidden),
            "provider source has legacy edge: {forbidden}"
        );
    }
    assert!(provider.contains("struct ProviderGateway"));
    assert!(provider.contains("impl ModelClient for ProviderGateway"));
    assert!(daemon.contains("kiana_provider::ProviderGateway::from_env"));
    let legacy_section = daemon
        .find("#[cfg(test)]\nmod legacy_fixtures")
        .expect("legacy services fixtures must stay test-only");
    for occurrence in daemon.match_indices("kiana_services") {
        assert!(
            occurrence.0 >= legacy_section,
            "legacy services runtime edge moved above cfg(test)"
        );
    }
    assert!(!runner_manifest.contains("kiana-provider"));
    assert!(!core_manifest.contains("kiana-provider"));
}
