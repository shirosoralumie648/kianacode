#[path = "support/source_guard.rs"]
mod source_guard;

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
    let domain_code = source_guard::production_source(domain);
    let core_code = source_guard::production_source(core);
    let daemon_code = source_guard::production_source(daemon);
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
            !domain_code.contains(forbidden)
                && !core_code.contains(forbidden)
                && !daemon_code.contains(forbidden),
            "DEP-10 startup path crossed an effect boundary: {forbidden}"
        );
    }
}
