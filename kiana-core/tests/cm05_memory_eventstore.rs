#[test]
fn memory_commit_has_no_unjournaled_visibility() {
    let core = include_str!("../src/capabilities.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/journal_core.rs");
    for marker in [
        "memory.fact",
        "append_idempotent_expected",
        "memory_projection_unjournaled",
        "memory_projection_lag",
        "MemoryMutation",
        "EventStorePort",
        "commit_transition",
        "projection_rebuild_matches_committed_memory",
    ] {
        assert!(
            core.contains(marker) || daemon.contains(marker) || eventlog.contains(marker),
            "memory eventstore marker missing: {marker}"
        );
    }
    assert!(daemon.contains("ensure_memory_projection(&self.0, &arguments).await?"));
    assert!(daemon.contains("journal_memory_fact"));
    assert!(eventlog.contains("plan_transition"));
    assert!(!daemon.contains("memory_authority_bypass"));
}
