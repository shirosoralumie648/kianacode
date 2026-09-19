#[test]
fn scenario_runner_requires_scrubbed_environment_and_cleanup_receipt() {
    let source = include_str!("../src/scenario.rs");
    assert!(source.contains("ScrubbedEnvironment"));
    assert!(source.contains("TraceDiff::compare"));
    assert!(source.contains("ScopePredicate"));
    assert!(source.contains("CleanupReceipt"));
    assert!(source.contains("scrubbed_environment_mismatch"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("PathBuf"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("CapabilityBrokerPort"));
}
