#[test]
fn canonicalizer_reuses_domain_canonical_and_redaction_boundaries() {
    let source = include_str!("../src/canonical.rs");
    assert!(source.contains("canonical_journal_bytes"));
    assert!(source.contains("redact_with_profile"));
    assert!(source.contains("CANONICAL_EVENT_FIELDS"));
    assert!(source.contains("ArrayPolicy::Multiset"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
}
