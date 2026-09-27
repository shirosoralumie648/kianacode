#[test]
fn dep09_supervisor_path_stays_narrow_and_fenced() {
    let domain = include_str!("../../kiana-domain/src/supervisor.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let core = include_str!("../src/deployment_supervisor.rs");
    let daemon = include_str!("../../kiana-daemon/src/supervisor_adapters.rs");
    for marker in [
        "SupervisorPort",
        "SupervisorRequest",
        "SupervisorObservation",
        "SupervisorBackend",
        "Systemd",
        "Launchd",
        "WindowsService",
        "Container",
        "expected_generation",
        "fence_token",
        "stop_report",
        "supervisor_stop_observation_required",
        "validate_supervisor_observation",
        "NarrowSupervisorAdapter",
    ] {
        assert!(
            domain.contains(marker)
                || ports.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker),
            "DEP-09 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "CapabilityRequest",
        "KianaHarness",
        "ModelClient",
        "std::process::Command",
        "tokio::process::Command",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden) && !daemon.contains(forbidden),
            "DEP-09 supervisor path crossed an execution boundary: {forbidden}"
        );
    }
}
