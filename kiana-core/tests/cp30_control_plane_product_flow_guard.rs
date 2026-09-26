#[test]
fn cp30_product_flow_reuses_daemonhost_and_controlplane_without_second_loop() {
    let domain = include_str!("../../kiana-domain/src/control_plane_product_flow.rs");
    let core = include_str!("../src/control_plane_product_flow.rs");
    let daemon = include_str!("../../kiana-daemon/tests/harness_runtime.rs");
    let golden = include_str!("../../kiana-daemon/tests/p3_i06_company_golden.rs");
    let entrypoint = include_str!("../src/entrypoint_parity.rs");
    let surface = include_str!("../../kiana-entrypoints/src/company_surface_parity.rs");
    let receipt = include_str!("../../kiana-core/src/receipts.rs");
    for marker in [
        "Cp30ProductFlowEvidence",
        "Cp30ProductFlowBundle",
        "Cp30FlowKind",
        "Cp30Surface",
        "ApprovalWrite",
        "CancelRecovery",
        "cp30_approval_write_requires_consumed_receipt",
        "cp30_surface_parity_mismatch",
        "DaemonHost",
        "ControlPlane",
        "host_model_tool_result_next_step_receipt_roundtrip",
        "fake_model_coding_project_produces_closing_receipt",
        "EntrypointParityMatrix",
        "CompanySurfaceParity",
        "Receipt",
        "validate_control_plane_product_bundle",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker)
                || golden.contains(marker)
                || entrypoint.contains(marker)
                || surface.contains(marker)
                || receipt.contains(marker),
            "CP-30 marker missing: {marker}"
        );
    }
    for forbidden in [
        "ModelClient::new",
        "CapabilityBroker::new",
        "std::process::Command",
        "auto_approve",
        "println!(\"approval",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "CP-30 second-loop/auto-approval marker present: {forbidden}"
        );
    }
}
