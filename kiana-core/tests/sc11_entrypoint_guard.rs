#[test]
fn entrypoint_parity_has_one_control_plane_route_and_no_local_authority_logic() {
    let parity = include_str!("../src/entrypoint_parity.rs");
    let domain = include_str!("../../kiana-domain/src/parity.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    for marker in [
        "EntrypointCommand",
        "EntrypointParityMatrix",
        "ENTRYPOINT_ROUTE",
        "DaemonHost->ControlPlane",
        "EntrypointDecision",
        "entrypoint_parity_denied_handler_effect",
        "context_digest",
    ] {
        assert!(
            parity.contains(marker),
            "entrypoint marker missing: {marker}"
        );
    }
    for marker in [
        "Cli",
        "Tty",
        "Web",
        "Workbench",
        "Desktop",
        "Scheduler",
        "Swarm",
        "Connector",
    ] {
        assert!(domain.contains(marker), "entrypoint kind missing: {marker}");
    }
    assert!(daemon.contains("pub async fn handle"));
    assert!(daemon.contains("SecurityContext"));
    for forbidden in [
        "allow_from_ui",
        "local_authority",
        "direct_broker",
        "handler(",
        "CapabilityBroker::new",
    ] {
        assert!(
            !parity.contains(forbidden),
            "parity contract must not contain {forbidden}"
        );
    }
}
