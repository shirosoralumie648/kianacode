#[test]
fn deletion_propagates_to_memory_and_index() {
    let domain = include_str!("../../kiana-domain/src/governance.rs");
    let core = include_str!("../src/data_governance.rs");
    let daemon = include_str!("../../kiana-daemon/src/data_governance.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let query = include_str!("../../kiana-daemon/src/context_query.rs");
    let checkpoint = include_str!("../src/workspace_checkpoints.rs");
    let baseline = include_str!("../../docs/roadmap/p2-k7-01-data-governance-baseline.md");

    for marker in [
        "DataClass",
        "Purpose",
        "ProcessingGrant",
        "Retention",
        "DataPolicy",
        "DataPayloadState",
        "DataGovernanceSnapshot",
        "DATA_GOVERNANCE_SNAPSHOT_SCHEMA",
        "data_epoch",
        "revoked_sources",
        "retention",
        "project_data_governance_snapshot",
        "data.revocation_requested",
        "workspace.restore_requested",
        "DataPayloadState::Unknown",
        "DataPayloadState::Revoked",
        "DataPayloadState::Expired",
        "derived_store_states",
        "data.governance",
        "action == \"delete\"",
        "action == \"expire\"",
        "policy.revoke",
        "purge_memory",
        "context-index.json",
        "context-artifacts.json",
        "context-artifact-store.json",
        "compaction",
        "cache_policy",
        "runner_snapshots",
        "data_revoked",
        "checkpoint_data_epoch_changed",
        "governance_revision_conflict",
        "governance_source_hardlink",
        "governance_path_symlink",
        "result_unknown",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker)
                || memory.contains(marker)
                || query.contains(marker)
                || checkpoint.contains(marker)
                || baseline.contains(marker),
            "data-governance marker missing: {marker}"
        );
    }

    for store in [
        "receipt", "audit", "artifact", "memory", "index", "cache", "export",
    ] {
        assert!(
            daemon.contains(&format!("\"{store}\":propagation_state"))
                || domain.contains(store)
                || baseline.contains(store),
            "propagation store missing: {store}"
        );
    }
    assert!(core.contains("if pending_invalidation"));
    assert!(core.contains("if !revoked_sources.is_empty()"));
    assert!(daemon.contains("Persist denial before touching derived files"));
    assert!(daemon.contains("Derived caches are disposable"));
    assert!(!daemon.contains("ModelClient"));
    assert!(!core.contains("CapabilityBroker"));
}
