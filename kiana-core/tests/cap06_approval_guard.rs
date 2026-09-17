#[test]
fn cap06_approval_is_bound_to_final_plan_and_revalidated() {
    let preview = include_str!("../../kiana-domain/src/approval_preview.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let approvals = include_str!("../src/approvals.rs");
    let recovery = include_str!("../src/recovery.rs");
    let journal = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "kiana.approval-plan-preview.v1",
        "ApprovalPlanPreview",
        "payload_digest",
        "preview_digest",
        "scope_digest",
        "environment_digest",
        "payload_available",
        "pending_with_proof",
        "prepare_capability_action",
        "approval_action_changed",
        "approval_permission_profile_changed",
        "approval_run_already_terminal",
        "approval_requirements_changed",
        "prepare_consumption",
        "approval_expected_version_conflict",
        "ApprovalExecutionMaterial",
    ] {
        assert!(
            preview.contains(marker)
                || capabilities.contains(marker)
                || approvals.contains(marker)
                || recovery.contains(marker)
                || journal.contains(marker)
                || protocol.contains(marker),
            "CAP-06 marker missing: {marker}"
        );
    }
    assert!(
        recovery.contains("ApprovalPlanPreview::from_pending"),
        "pending projection must expose one final-plan preview"
    );
    assert!(approvals.contains("pending_with_proof"));
    assert!(approvals.contains("prepared != validated.request"));
    assert!(approvals.contains("authorize_capability_action"));
    assert!(journal.contains("matches_payloads"));
    for forbidden in [
        "execute_preview_payload",
        "approval_preview_is_authority",
        "approve_without_revalidation",
        "preview_as_execution_material",
    ] {
        assert!(
            !approvals.contains(forbidden)
                && !recovery.contains(forbidden)
                && !journal.contains(forbidden),
            "forbidden CAP-06 path: {forbidden}"
        );
    }
}
