#[test]
fn core_correction_boundary_has_no_model_or_second_authority() {
    let source = include_str!("../src/cost_correction.rs");
    assert!(source.contains("CostCorrectionAdmission"));
    assert!(source.contains("RuntimeEvent"));
    assert!(source.contains("ControlPlane"));
    assert!(!source.contains("ModelOutput"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("reqwest"));
}
