#[test]
fn aut23_surface_uat_reuses_the_daemonhost_spine_and_denies_direct_effects() {
    let domain = include_str!("../../kiana-domain/src/automation_surface_uat.rs");
    let core = include_str!("../src/automation_surface_uat.rs");
    let snapshot = include_str!("../src/automation_snapshot.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "AutomationSurfaceUat",
        "AutomationSurfaceUatCase",
        "AutomationUatSurface",
        "Mcp",
        "snapshot_digest",
        "source_cursor",
        "authority_epoch",
        "direct_route",
        "DaemonHost",
        "ControlPlane",
        "validate_automation_surface_uat",
        "AutomationSnapshot",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || snapshot.contains(marker)
                || daemon.contains(marker)
                || protocol.contains(marker),
            "AUT-23 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "direct_broker",
        "auto_approve",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-23 direct-effect marker present: {forbidden}"
        );
    }
}
