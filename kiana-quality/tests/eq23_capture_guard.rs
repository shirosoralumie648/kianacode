#[test]
fn capture_requires_explicit_source_and_immutable_logical_destination() {
    let source = include_str!("../src/capture.rs");
    assert!(source.contains("CaptureSource::Run"));
    assert!(source.contains("CaptureSource::Fixture"));
    assert!(source.contains("DestinationExists"));
    assert!(source.contains("GoldenTrace::new"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("PathBuf"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("CapabilityBrokerPort"));
}
