//! OA-20 source guard: data governance is a projection/propagation boundary bound to one
//! project, not a second authority, an EventLog rewrite path or a UI/cache-only cleanup.

#[test]
fn oa20_settles_every_payload_store_and_refuses_scope_or_cache_only_claims() {
    let domain = include_str!("../../kiana-domain/src/governance.rs");
    let core = include_str!("../src/data_governance.rs");
    let deletion = include_str!("../src/deletion.rs");
    let daemon = include_str!("../../kiana-daemon/src/data_governance.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/governance_contract.rs");
    let artifact_store = include_str!("../../kiana-eventlog/src/artifact_store.rs");
    let memory_store = include_str!("../../kiana-eventlog/src/memory_store.rs");
    let retention_store = include_str!("../../kiana-eventlog/src/retention_store.rs");
    let index = include_str!("../../kiana-query/src/index_invalidation.rs");
    let domain_fixture = include_str!("../../kiana-domain/tests/oa20_data_governance.rs");
    let core_fixture = include_str!("../tests/oa20_data_governance_projection.rs");
    let baseline = include_str!("../../docs/roadmap/observability-audit-baseline.md");
    let workflow = include_str!("../../.github/workflows/oa20-data-governance.yml");

    // Domain contract: every payload-bearing target has to be settled before coverage is claimed,
    // and a settled receipt must bind the exact plan boundary.
    for marker in [
        "require_payload_coverage",
        "require_complete",
        "unresolved_payload_targets",
        "data_propagation_payload_coverage_incomplete",
        "data_propagation_targets_unresolved",
        "data_propagation_receipt_binding_mismatch",
        "data_propagation_receipt_not_settled",
        "fn settle(",
        "fn settled(",
        "holds_payload",
        "audit_metadata_retained",
        "derived_store_states",
    ] {
        assert!(
            domain.contains(marker),
            "OA-20 domain contract marker missing: {marker}"
        );
    }

    // Core projection: project scope is decided server-side and a foreign or unattributed
    // invalidation fact is refused instead of being applied to the asking project.
    for marker in [
        "governance_fact_matches_project",
        "data_governance_project_scope_mismatch",
        "data_governance_project_root_missing",
        "data_governance_source_cursor_gap",
        "data_governance_source_event_duplicate",
        "project_memory_deletion_index",
        "apply_memory_deletion_surface_states",
        "receipt_data_binding_from_events",
        "seal_governance_events",
        "plan_data_propagation",
        "data.revocation_requested",
        "workspace.restore_requested",
        "kiana.data-governance-result.v1",
    ] {
        assert!(
            core.contains(marker),
            "OA-20 core projection marker missing: {marker}"
        );
    }

    // Deletion planning stays receipt-bound: every target starts Unknown and the manifest is
    // explicit about what still needs adapter evidence.
    for marker in [
        "plan_deletion_propagation",
        "deletion_governance_snapshot_stale",
        "adapter_receipt_required",
    ] {
        assert!(
            deletion.contains(marker),
            "OA-20 deletion planning marker missing: {marker}"
        );
    }

    // Daemon purge stays derived-store-only; the EventLog is never rewritten and the operator
    // gate is required before any local byte moves.
    for marker in [
        "governance_operator_required",
        "processing_source_changed",
        "read_policy",
        "read_deletion_index",
        "purge_memory",
        "fence_purged_entry",
        "memory-deletion-index.json",
        "historical_receipts",
        "propagation",
    ] {
        assert!(
            daemon.contains(marker),
            "OA-20 daemon propagation marker missing: {marker}"
        );
    }

    for marker in [
        "data.propagation_receipt",
        "data.immutable_event_seal",
        "eventlog_immutable_event_seal_payload_boundary",
    ] {
        assert!(
            eventlog.contains(marker),
            "OA-20 governance event boundary marker missing: {marker}"
        );
    }

    // Derived stores keep their own invalidation receipts; none of them may claim completion
    // merely because a receipt was redacted.
    for marker in [
        "artifact_data_revoked_or_expired",
        "invalidate_artifact",
        "append_propagation_receipt",
        "MemoryDataGovernanceStore",
        "read_allowed",
        "governed_index_invalidation",
        "index_data_epoch_stale",
    ] {
        assert!(
            artifact_store.contains(marker)
                || memory_store.contains(marker)
                || retention_store.contains(marker)
                || index.contains(marker),
            "OA-20 derived store marker missing: {marker}"
        );
    }

    for fixture in [
        "cache_only_cleanup_cannot_claim_payload_coverage",
        "receipt_binding_drift_and_unset_receipts_are_refused_before_any_slot_changes",
        "raw_payload_and_spool_bytes_cannot_be_decoded_into_governance_dtos",
        "retained_audit_metadata_never_authorizes_payload_reads",
        "expired_payload_keeps_metadata_separate_and_leaves_payload_targets_open",
        "settled_payload_targets_are_complete_and_digest_stable",
    ] {
        assert!(
            domain_fixture.contains(fixture),
            "OA-20 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "foreign_or_unattributed_project_scope_facts_are_refused",
        "deletion_propagation_plan_keeps_every_store_open_until_settled",
        "receipt_binding_separates_payload_refs_from_audit_metadata_and_epoch",
        "rebuild_from_the_same_committed_facts_is_stable_across_restarts",
    ] {
        assert!(
            core_fixture.contains(fixture),
            "OA-20 core fixture missing: {fixture}"
        );
    }

    for target in [
        "cargo test -p kiana-domain --test oa20_data_governance",
        "cargo test -p kiana-core --test oa20_data_governance_projection",
        "cargo test -p kiana-core --test oa20_data_governance_guard",
    ] {
        assert!(
            workflow.contains(target),
            "OA-20 focused lane missing target: {target}"
        );
    }
    assert!(baseline.contains("require_payload_coverage") || baseline.contains("OA-20"));

    // No second execution loop, no EventLog rewrite and no raw payload path in the projection.
    assert!(!core.contains("CapabilityBroker::new"));
    assert!(!core.contains("KianaHarness::new"));
    assert!(!core.contains("raw_payload"));
    assert!(!domain.contains("raw_payload"));
}

#[test]
fn oa20_governance_dtos_reject_raw_payload_fields_at_the_wire_boundary() {
    let domain = include_str!("../../kiana-domain/src/governance.rs");
    // Every governance DTO that can carry a reference keeps `deny_unknown_fields`, so a spool
    // body, prompt echo or trace blob cannot be smuggled into a propagation receipt.
    assert!(domain.matches("deny_unknown_fields").count() >= 8);
    for dto in [
        "pub struct DataGovernanceSnapshot",
        "pub struct DataPropagationPlan",
        "pub struct DataPropagationReceipt",
        "pub struct ReceiptDataBinding",
        "pub struct ReceiptPayloadRef",
        "pub struct ReceiptAuditMetadata",
    ] {
        assert!(domain.contains(dto), "OA-20 governance DTO missing: {dto}");
    }
}
