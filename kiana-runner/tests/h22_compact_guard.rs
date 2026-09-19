#[test]
fn h22_compaction_keeps_summary_evidence_and_complete_recent_groups() {
    let compact = include_str!("../src/compact.rs");
    let harness = include_str!("../src/harness.rs");
    let domain = include_str!("../../kiana-domain/src/compact_summary.rs");
    for marker in [
        "CompactSummary",
        "build_compacted_history_with_summary",
        "message_groups",
        "pending assistant/tool pair preserved",
        "compact_latest_group_exceeds_budget",
        "completed_action_refs",
        "verification_refs",
        "summary_digest",
    ] {
        assert!(
            compact.contains(marker) || harness.contains(marker) || domain.contains(marker),
            "H22 compaction marker missing: {marker}"
        );
    }
    assert!(!compact.contains("(no summary available)"));
    assert!(compact.contains("Product-owned system instructions are an immutable prefix"));
    assert!(domain.contains("compact_summary_evidence_ref_invalid"));
}
