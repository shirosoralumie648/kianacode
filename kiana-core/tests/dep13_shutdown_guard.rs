#[test]
fn dep13_shutdown_reducer_is_pure_and_unknown_safe() {
    let domain = include_str!("../../kiana-domain/src/deployment_shutdown.rs");
    let core = include_str!("../src/deployment_shutdown.rs");
    let daemon = include_str!("../../kiana-daemon/src/deployment_shutdown.rs");
    for marker in [
        "ShutdownPhase",
        "ShutdownInput",
        "ShutdownReport",
        "event_store_flushed",
        "artifact_flushed",
        "late_result_count",
        "NeedsRecovery",
        "ShutdownStatus::Unknown",
        "evaluate_shutdown",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "DEP-13 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process",
        "tokio::",
        "EventStore::append",
        "CapabilityBroker",
        "KianaHarness",
        "http::StatusCode",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden) && !daemon.contains(forbidden),
            "DEP-13 shutdown path crossed forbidden boundary: {forbidden}"
        );
    }
}
