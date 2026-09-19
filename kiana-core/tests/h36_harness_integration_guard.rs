//! H36 source guard for the harness integration matrix.

#[test]
fn h36_matrix_keeps_surface_scenario_and_receipt_boundaries() {
    let source = include_str!("../../kiana-domain/src/harness_integration.rs");
    let baseline = include_str!("../../docs/roadmap/h36-harness-integration-baseline.md");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let provider_live = include_str!("../../kiana-domain/src/provider_live.rs");
    let live_handoff = include_str!("../../kiana-domain/src/live_handoff.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");
    for marker in [
        "HarnessSurface",
        "Cli",
        "Workbench",
        "Web",
        "Desktop",
        "HarnessScenario",
        "ShortTask",
        "ToolCall",
        "Repair",
        "Steer",
        "Cancel",
        "Approval",
        "Compaction",
        "Restart",
        "receipt_digest",
        "cancel_fence_verified",
        "stream_evidence",
        "desktop_state_receipt",
        "result_unknown",
        "harness_integration_coverage_missing",
        "live_closeout_blockers",
        "live_closeout_ready",
        "harness_integration_verified_unknown_conflict",
    ] {
        assert!(
            source.contains(marker),
            "H36 matrix marker missing: {marker}"
        );
    }
    for marker in [
        "three-entry",
        "Desktop",
        "short-task",
        "repair",
        "steer",
        "approval",
        "compaction",
        "restart",
        "partial",
        "real Provider",
        "receipt",
        "live_closeout_blockers",
        "live_closeout_ready",
        "ProviderLiveConnectionEvidence",
        "LiveHandoffManifest",
        "DaemonHost",
        "ControlPlane",
        "ProviderGateway",
        "without external credentials",
    ] {
        assert!(
            baseline.contains(marker),
            "H36 baseline marker missing: {marker}"
        );
    }
    for (name, text) in [
        ("daemon", daemon),
        ("runner", runner),
        ("provider", provider),
        ("provider_live", provider_live),
        ("live_handoff", live_handoff),
        ("cli", cli),
        ("workbench", workbench),
        ("web", web),
        ("desktop", desktop),
    ] {
        assert!(!text.is_empty(), "H36 source is empty: {name}");
    }
    for marker in [
        "DaemonHost",
        "ControlPlane",
        "KianaHarness",
        "ProviderGateway",
        "ProviderLiveConnectionEvidence",
        "LiveHandoffManifest",
        "startHarness",
    ] {
        assert!(
            daemon.contains(marker)
                || runner.contains(marker)
                || provider.contains(marker)
                || provider_live.contains(marker)
                || live_handoff.contains(marker)
                || cli.contains(marker)
                || workbench.contains(marker)
                || web.contains(marker)
                || desktop.contains(marker),
            "H36 shared-spine marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
