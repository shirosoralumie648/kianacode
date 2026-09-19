//! H36 source guard for the three-entry DaemonHost spine and live-provider boundary.

#[test]
fn harness_integration_keeps_entrypoints_on_one_spine_and_live_opt_in() {
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let core = include_str!("../src/lib.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");
    let handoff = include_str!("../../kiana-domain/src/live_handoff.rs");
    let baseline = include_str!("../../docs/roadmap/h36-harness-integration-baseline.md");

    for (label, source) in [
        ("cli", cli),
        ("workbench", workbench),
        ("web", web),
        ("desktop", desktop),
    ] {
        assert!(
            source.contains("DaemonHost") || source.contains("daemon"),
            "H36 {label} does not identify the daemon spine"
        );
    }
    for marker in [
        "ControlPlane",
        "DaemonHost",
        "KianaHarness",
        "ProviderGateway",
        "cancel_run",
        "result_unknown",
        "run_stream",
    ] {
        assert!(
            daemon.contains(marker)
                || core.contains(marker)
                || harness.contains(marker)
                || provider.contains(marker),
            "H36 product marker missing: {marker}"
        );
    }
    for marker in [
        "LiveHandoffManifest",
        "NotSupported",
        "operator_approval_ref",
        "provider_receipt_ref",
        "cleanup_plan",
    ] {
        assert!(
            handoff.contains(marker),
            "H36 live marker missing: {marker}"
        );
    }
    for marker in [
        "three-entry",
        "DaemonHost",
        "cassette",
        "real Provider",
        "live",
        "result_unknown",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "H36 baseline marker missing: {marker}"
        );
    }
    assert!(!cli.contains("CapabilityBroker::new"));
    assert!(!workbench.contains("CapabilityBroker::new"));
    assert!(!web.contains("CapabilityBroker::new"));
}
