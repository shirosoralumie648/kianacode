#[test]
fn trace_diff_is_pure_and_reports_bounded_summaries() {
    let source = include_str!("../src/diff.rs");
    assert!(source.contains("first_json_divergence"));
    assert!(source.contains("TraceDiffClass::FieldMismatch"));
    assert!(source.contains("redact_text"));
    assert!(source.contains("MAX_SUMMARY_BYTES"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
    assert!(!source.contains("CapabilityBrokerPort"));
}
