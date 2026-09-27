#[test]
fn dep09_daemon_adapters_do_not_execute_or_fallback() {
    let source = include_str!("../src/supervisor_adapters.rs");
    for marker in [
        "SupervisorBackend::Systemd",
        "SupervisorBackend::Launchd",
        "SupervisorBackend::WindowsService",
        "SupervisorBackend::Container",
        "supervisor_backend_binding_mismatch",
        "supervisor_action_binding_mismatch",
        "adapter_unavailable",
    ] {
        assert!(
            source.contains(marker),
            "DEP-09 adapter marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::process::Command",
        "tokio::process::Command",
        "CapabilityBroker",
        "KianaHarness",
        "pid:",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-09 adapter contains forbidden fallback/effect marker: {forbidden}"
        );
    }
}
