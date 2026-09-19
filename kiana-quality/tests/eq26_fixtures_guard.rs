#[test]
fn fixture_matrix_is_provider_independent_and_has_no_effect_path() {
    let source = include_str!("../src/fixtures.rs");
    assert!(source.contains("FixtureFamily::Runtime"));
    assert!(source.contains("FixtureFamily::Approval"));
    assert!(source.contains("FixtureFamily::Hook"));
    assert!(source.contains("FixtureFamily::Memory"));
    assert!(source.contains("FixtureFamily::Workflow"));
    assert!(source.contains("FixtureFamily::Swarm"));
    assert!(source.contains("provider_calls"));
    assert!(source.contains("forbidden_effects"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
    assert!(!source.contains("CapabilityBrokerPort"));
}
