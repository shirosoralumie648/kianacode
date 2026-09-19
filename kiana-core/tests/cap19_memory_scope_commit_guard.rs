//! CAP-19 source guard for server-owned memory scope and journal-before-projection writes.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-19 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn memory_adapter_uses_server_scope_and_reliable_commit_boundary() {
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let retrieval = include_str!("../../kiana-daemon/src/memory_retrieval.rs");
    let governance = include_str!("../../kiana-daemon/src/data_governance.rs");
    let domain = include_str!("../../kiana-domain/src/context_scope.rs");
    let core = include_str!("../src/capabilities.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");

    require(
        memory,
        &[
            "MemoryScope::capture",
            "memory_storage_scope_changed",
            "server_memory_scope",
            "request.request.execution_scope",
            "DomainMemoryScope::from_execution_scope",
            "scope.allows_collection",
            "collection_path_scoped",
            "ensure_memory_projection",
            "memory_projection_unjournaled",
            "memory_projection_lag",
            "journal_memory_fact",
            "append_idempotent_expected",
            "MemoryMutationLedger",
            "MemoryAdmission::Candidate",
            "memory_review_operator_required",
        ],
        "memory adapter",
    );
    require(
        retrieval,
        &[
            "memory_hits_from_events",
            "memory_match_terms",
            "role_knowledge_denied",
        ],
        "memory retrieval",
    );
    require(
        governance,
        &[
            "governance_path_symlink",
            "data_epoch",
            "purge_memory",
            "revoked_sources",
        ],
        "memory governance",
    );
    require(
        domain,
        &[
            "pub fn intersect",
            "memory_scope_intersection_empty",
            "allows_collection",
        ],
        "domain scope",
    );
    require(
        core,
        &[
            "memory.search",
            "memory.write",
            "memory.review",
            "execution_scope",
        ],
        "control-plane boundary",
    );
    require(
        baseline,
        &[
            "memory_scope_cannot_be_replaced_by_model_arguments",
            "memory_store_symlink_or_namespace_escape_is_rejected",
            "memory_cancel_does_not_report_success_before_writer_stops",
        ],
        "CAP-19 card",
    );
    for forbidden in ["memory_authority_bypass", "memory_store_root_from_model"] {
        assert!(
            !memory.contains(forbidden),
            "CAP-19 bypass marker present: {forbidden}"
        );
    }
}
