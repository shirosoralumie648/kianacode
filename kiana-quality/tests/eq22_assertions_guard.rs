#[test]
fn assertion_dsl_is_pure_and_has_bounded_regex_and_summary_inputs() {
    let source = include_str!("../src/assertions.rs");
    assert!(source.contains("AssertionMode::Exact"));
    assert!(source.contains("AssertionMode::Ordered"));
    assert!(source.contains("AssertionMode::Multiset"));
    assert!(source.contains("NumericTolerance"));
    assert!(source.contains("Regex::new"));
    assert!(source.contains("MAX_REGEX_BYTES"));
    assert!(source.contains("MAX_ASSERTION_SUMMARY_BYTES"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
    assert!(!source.contains("CapabilityBrokerPort"));
}
