#[test]
fn persistence_baseline_keeps_facts_projections_and_caches_separate() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/lib.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let memory = include_str!("../../kiana-eventlog/src/memory.rs");
    let artifact = include_str!("../src/artifacts.rs");
    let query = include_str!("../../kiana-query/src/index.rs");
    let baseline = include_str!("../../docs/roadmap/persistence-data-layer-baseline.md");

    assert!(ports.contains("trait EventStorePort"));
    assert!(ports.contains("commit_transition"));
    assert!(ports.contains("read_from"));
    assert!(eventlog.contains("JsonlEventLog"));
    assert!(eventlog.contains("MemoryEventLog"));
    assert!(jsonl.contains("JOURNAL_HEADER_SCHEMA"));
    assert!(jsonl.contains("MAX_JOURNAL_LOG_BYTES"));
    assert!(jsonl.contains("sync_all"));
    assert!(memory.contains("durable_commits: false"));
    assert!(artifact.contains("artifact"));
    assert!(query.contains("generation") || query.contains("content_hash"));
    for required in [
        "single fact source",
        "Projection/Cache",
        "StorageRoot",
        "MemoryEventLog",
        "JsonlEventLog",
        "ApprovalStore",
        "Artifact",
        "backup",
        "migration",
        "retention",
    ] {
        assert!(baseline.contains(required), "baseline missing {required}");
    }
}

#[test]
fn persistence_baseline_rejects_inferred_durability_or_second_store_authority() {
    let baseline = include_str!("../../docs/roadmap/persistence-data-layer-baseline.md");
    assert!(baseline.contains("不能把 Memory 标成 durable"));
    assert!(baseline.contains("不允许双写两套事实"));
    assert!(baseline.contains("proof ceiling"));
    assert!(baseline.contains("PD-01"));
    assert!(baseline.contains("PD-05"));
}
