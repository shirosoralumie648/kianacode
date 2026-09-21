#[test]
fn memory_search_uses_projection_fence_and_exposes_epoch() {
    let daemon = include_str!("../src/harness_memory.rs");
    let core = include_str!("../../kiana-core/src/data_governance.rs");
    for marker in [
        "MemoryProjectionFence::evaluate",
        "data_policy.data_epoch",
        "source_revoked_or_retention_expired",
        "\"data_epoch\":data_policy.data_epoch",
    ] {
        assert!(
            daemon.contains(marker) || core.contains(marker),
            "missing marker: {marker}"
        );
    }
}
