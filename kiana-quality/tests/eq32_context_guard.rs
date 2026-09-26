#[test]
fn context_memory_evaluator_stays_pure_and_acl_first() {
    let source = include_str!("../src/context.rs");
    for forbidden in [
        "std::fs",
        "tokio::",
        "reqwest::",
        "Provider",
        "MemoryStore",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "Command::new",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden context evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "CONTEXT_MEMORY_INPUT_SCHEMA",
        "ContextMemoryEvaluator",
        "acl_checked_before_ranking",
        "context.unauthorized_memory_hit",
        "context.provenance_invalid",
        "context.freshness_unknown",
        "context.compaction_integrity_invalid",
        "context.budget_overflow",
        "context.budget_unenforced",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-32 boundary marker: {required}"
        );
    }
}
