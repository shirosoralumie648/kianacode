#[test]
fn cp09_approval_material_keeps_preview_non_executable_and_rechecks_authority() {
    let journal = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let material = include_str!("../../kiana-domain/src/approval_journal.rs");
    let approvals = include_str!("../src/approvals.rs");
    for marker in [
        "ApprovalExecutionMaterial",
        "ApprovalMaterialState",
        "VolatileProtected",
        "InlineRedacted",
        "approval_payload_unrecoverable",
        "approval_preview_redaction_unstable",
        "approval_nonce_unavailable",
        "material.matches_payloads",
        "subject.material.validate",
    ] {
        assert!(
            journal.contains(marker) || material.contains(marker),
            "CP-09 marker missing: {marker}"
        );
    }
    assert!(journal.contains("check_authority"));
    assert!(approvals.contains("prepare_capability_action"));
    assert!(approvals.contains("pending_invocations"));
    for forbidden in ["raw secret value", "direct Broker", "authorize_and_execute"] {
        assert!(
            !material.contains(forbidden),
            "approval material must not contain {forbidden}"
        );
    }
}
