#[test]
fn er32_adapter_conformance_preserves_memory_jsonl_and_unknown_boundaries() {
    let domain = include_str!("../../kiana-domain/src/er32_adapter_conformance.rs");
    let core = include_str!("../src/er32_adapter_conformance.rs");
    let memory = include_str!("../../kiana-eventlog/src/memory.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let journal = include_str!("../../kiana-eventlog/src/journal_core.rs");
    for marker in [
        "Er32ConformanceReport",
        "Er32AdapterObservation",
        "Er32AdapterKind",
        "Memory",
        "Jsonl",
        "er32_memory_cannot_claim_durable",
        "er32_replay_result_invalid",
        "er32_unknown_result_invalid",
        "CommitOutcome",
        "TransitionPlan",
        "validate_er32_conformance_report",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || memory.contains(marker)
                || jsonl.contains(marker)
                || journal.contains(marker),
            "ER-32 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::process::Command",
        "CapabilityBroker::new",
        "ModelClient::new",
        "append_event",
        "write_all",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "ER-32 adapter/effect bypass marker present: {forbidden}"
        );
    }
}
