//! H36 source guard for the harness integration matrix.

#[test]
fn h36_matrix_keeps_surface_scenario_and_receipt_boundaries() {
    let source = include_str!("../../kiana-domain/src/harness_integration.rs");
    let baseline = include_str!("../../docs/roadmap/h36-harness-integration-baseline.md");
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
    ] {
        assert!(
            baseline.contains(marker),
            "H36 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
