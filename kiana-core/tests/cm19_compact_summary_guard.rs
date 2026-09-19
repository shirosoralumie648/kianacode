#[test]
fn compact_summary_keeps_evidence_and_complete_recent_groups() {
    let domain = include_str!("../../kiana-domain/src/compact_summary.rs");
    let compact = include_str!("../../kiana-runner/src/compact.rs");
    for marker in [
        "CompactSummaryEvidence",
        "CompactEvidenceStatus",
        "validate_against_evidence",
        "compact_summary_completed_fact_not_confirmed",
        "compact_summary_decision_fact_not_confirmed",
        "build_compacted_history_with_summary",
        "message_groups",
        "pending-call",
        "compact_latest_group_exceeds_budget",
    ] {
        assert!(
            domain.contains(marker) || compact.contains(marker),
            "CM-19 source marker missing: {marker}"
        );
    }
    assert!(!compact.contains("(no summary available)"));
    assert!(!domain.contains("ModelClient"));
    assert!(!domain.contains("CapabilityBroker"));
}
