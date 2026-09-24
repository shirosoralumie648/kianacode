#[test]
fn connector_risk_policy_gate_approval_stays_on_control_plane_path() {
    let domain = include_str!("../../kiana-domain/src/connector_policy.rs");
    let policy = include_str!("../../kiana-policy/src/lib.rs");
    let gates = include_str!("../../kiana-gates/src/lib.rs");
    let core = include_str!("../src/connectors.rs");
    let approvals = include_str!("../src/approvals.rs");
    for marker in [
        "ConnectorOperationRisk",
        "R0ReadOnly",
        "R1ReadOnly",
        "R2DataGrant",
        "R3OnceApproval",
        "R4DefaultDeny",
        "ConnectorDataGrant",
        "ConnectorOnceApproval",
        "evaluate_connector_admission",
        "connector_policy_decision",
        "connector_r4_default_denied",
        "connector_final_payload_digest",
        "connector_binding_revoked",
        "connector_policy_epoch_mismatch",
        "connector_authority_epoch_mismatch",
        "connector_final_payload_approval_required",
        "final_payload_digest",
        "authorize_and_execute",
        "stage_capability_action",
        "decide_approval_with_proof",
    ] {
        assert!(
            domain.contains(marker)
                || policy.contains(marker)
                || gates.contains(marker)
                || core.contains(marker)
                || approvals.contains(marker),
            "missing INT-15 marker: {marker}"
        );
    }
    assert!(core.contains("self.authorize_and_execute(&context, request).await"));
    assert!(core.contains("connector_payload_sha256(payload)"));
    assert!(policy.contains("evaluate_connector_admission"));
    assert!(policy.contains("DefaultPolicyEngine"));
    assert!(approvals.contains("stage_capability_action"));
    assert!(approvals.contains("decide_approval_with_proof"));
    assert!(!core.contains("CapabilityBroker"));
    assert!(!policy.contains("CapabilityBroker"));
}
