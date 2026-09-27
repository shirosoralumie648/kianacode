#[test]
fn dep16_reconcile_is_explicit_read_only_and_fenced() {
    let domain = include_str!("../../kiana-domain/src/deployment_reconcile.rs");
    let core = include_str!("../src/deployment_reconcile.rs");
    for marker in [
        "RepairTarget",
        "RepairFact",
        "ReconcileInput",
        "ReconcileReport",
        "CommitReady",
        "reconcile_result_unknown",
        "planned_projection_generation",
        "explicit_commit_required",
        "evaluate_reconcile",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-16 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "EventStore::append",
        "ProjectionStorePort",
        "WorkflowQueueStore",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::",
        "auto_retry",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-16 reconcile crossed effect boundary: {forbidden}"
        );
    }
}
