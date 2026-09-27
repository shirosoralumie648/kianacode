#[test]
fn dep05_operation_lifecycle_is_pure_and_replayable() {
    let domain = include_str!("../../kiana-domain/src/operation_lifecycle.rs");
    let core = include_str!("../src/deployment_operation.rs");
    for marker in [
        "OperationJournal",
        "OperationTransition",
        "OperationTransitionKind",
        "PhaseDeadline",
        "operation_preflight_required",
        "operation_revision_late_event",
        "operation_drain_timeout_not_due",
        "operation_terminal_immutable",
        "replay_operation_journal",
        "source_cursor",
        "evidence_refs",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-05 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "dispatch_capability",
        "start_revision",
        "stop_revision",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-05 lifecycle path crossed an effect boundary: {forbidden}"
        );
    }
}
