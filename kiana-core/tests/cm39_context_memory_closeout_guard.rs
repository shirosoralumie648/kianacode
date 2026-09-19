//! CM-39 source guard for context/memory documentation and handoff parity.

#[test]
fn context_memory_docs_name_sources_proof_ceiling_and_open_limits() {
    let roadmap = include_str!("../../docs/roadmap/context-memory.md");
    let module_map = include_str!("../../docs/module-map.md");
    let status = include_str!("../../CURRENT_STATUS.md");
    let baseline = include_str!("../../docs/roadmap/cm39-context-memory-closeout-baseline.md");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let retrieval = include_str!("../../kiana-daemon/src/memory_retrieval.rs");
    let journal = include_str!("../../kiana-domain/src/memory_journal.rs");

    for marker in [
        "CM-38",
        "CM-39",
        "context_memory_golden_path_is_durable",
        "live_provider_evidence_keeps_scope_and_redaction",
        "provider",
        "跨进程",
        "physical",
        "scale",
    ] {
        assert!(
            roadmap.contains(marker)
                || module_map.contains(marker)
                || status.contains(marker)
                || baseline.contains(marker),
            "CM-39 documentation marker missing: {marker}"
        );
    }
    for marker in [
        "memory.search",
        "memory.write",
        "candidate",
        "review",
        "EventStore",
        "scope",
    ] {
        assert!(
            memory.contains(marker) || retrieval.contains(marker) || journal.contains(marker),
            "CM-39 source marker missing: {marker}"
        );
    }
    for marker in [
        "documentation_matches_source_and_receipt_contracts",
        "CURRENT_STATUS",
        "module-map",
        "open limitations",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "CM-39 baseline marker missing: {marker}"
        );
    }
}
