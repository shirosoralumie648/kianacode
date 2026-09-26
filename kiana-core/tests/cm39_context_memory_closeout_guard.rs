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
    let evidence = include_str!("../../kiana-domain/src/context_memory_evidence.rs");

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
        "ContextMemoryGoldenPathEvidence",
        "scope_digest",
        "redaction_profile_digest",
        "FakeCassette",
        "LiveOptIn",
        "source-only",
        "durable",
    ] {
        assert!(
            memory.contains(marker)
                || retrieval.contains(marker)
                || journal.contains(marker)
                || evidence.contains(marker),
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

    let cm38_start = status
        .find("### CM-38 live evidence approval identity fence")
        .expect("CM-38 status block");
    let cm38_tail = &status[cm38_start..];
    let cm38_end = cm38_tail.find("\n### ").unwrap_or(cm38_tail.len());
    let cm38_block = &cm38_tail[..cm38_end];
    for field in [
        "source_snapshot:",
        "worktree_status:",
        "command_argv:",
        "cwd·environment:",
        "fixture·cassette:",
        "exit_code:",
        "status_change:",
        "proof-level change:",
        "limitations:",
        "reviewer:",
    ] {
        assert!(
            cm38_block.contains(field),
            "CM-38 evidence field missing: {field}"
        );
    }
    assert!(cm38_block.contains("proof_level=source"));
    assert!(cm38_block.contains("未提升 local_behavior/durable/live/physical"));
}
