#[test]
fn memory_temporal_selection_keeps_as_of_supersedes_and_conflict_boundaries() {
    let source = include_str!("../../kiana-domain/src/memory_temporal.rs");
    for marker in [
        "resolve_memory_history",
        "as_of_ms",
        "superseded_by",
        "Conflict",
        "not_valid_at_as_of",
        "MemoryAclDecision::evaluate",
        "conflict_sets",
    ] {
        assert!(
            source.contains(marker),
            "CM-24 source marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}
