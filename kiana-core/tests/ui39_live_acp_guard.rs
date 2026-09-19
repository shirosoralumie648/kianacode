//! UI-39 source guard for live ACP/IDE opt-in boundaries.

#[test]
fn live_acp_ide_is_opt_in_and_host_capabilities_stay_server_owned() {
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let entrypoints = include_str!("../../kiana-entrypoints/src/web.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");
    let handoff = include_str!("../../kiana-domain/src/live_handoff.rs");
    let baseline = include_str!("../../docs/roadmap/ui39-live-acp-baseline.md");

    for marker in [
        "UiHandshake",
        "UiSnapshot",
        "UiAction",
        "UiCapability",
        "DaemonHost",
        "ControlPlane",
        "session",
        "cancel",
        "epoch",
        "cursor",
    ] {
        assert!(
            protocol.contains(marker)
                || client.contains(marker)
                || daemon.contains(marker)
                || entrypoints.contains(marker)
                || workbench.contains(marker),
            "UI-39 shared marker missing: {marker}"
        );
    }
    for marker in [
        "LiveHandoffManifest",
        "NotSupported",
        "OptedIn",
        "operator_approval_ref",
    ] {
        assert!(
            handoff.contains(marker),
            "UI-39 live marker missing: {marker}"
        );
    }
    for marker in [
        "live",
        "opt-in",
        "ACP",
        "IDE",
        "host",
        "ControlPlane",
        "partial",
        "not_supported",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-39 baseline marker missing: {marker}"
        );
    }
    assert!(desktop.contains("DaemonHost") || desktop.contains("worker"));
    assert!(!entrypoints.contains("CapabilityBroker::new"));
    assert!(!workbench.contains("CapabilityBroker::new"));
}
