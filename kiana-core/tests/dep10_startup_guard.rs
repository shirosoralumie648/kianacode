#[test]
fn dep10_startup_coordinator_is_pure_and_ordered() {
    let domain = include_str!("../../kiana-domain/src/deployment_startup.rs");
    let core = include_str!("../src/deployment_startup.rs");
    let daemon = include_str!("../../kiana-daemon/src/startup_coordinator.rs");
    for marker in [
        "StartupStage::ALL",
        "Manifest",
        "Root",
        "Trust",
        "Lease",
        "Store",
        "Migration",
        "Projector",
        "Capacity",
        "StartupCoordinatorRequest",
        "StartupCoordinatorReport",
        "startup_journal_invalid",
        "startup_resume_requires_explicit_recovery",
        "validate_startup_report",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "DEP-10 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "tokio::",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "MigrationRunnerPort",
        "ProjectionStorePort",
        "SupervisorPort",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden) && !daemon.contains(forbidden),
            "DEP-10 startup path crossed an effect boundary: {forbidden}"
        );
    }
}
