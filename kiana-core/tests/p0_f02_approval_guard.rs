#[test]
fn approval_decision_is_durable_and_single_use() {
    let approvals = include_str!("../src/approvals.rs");
    let journal = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let existing = include_str!("control_plane.rs");

    for marker in [
        "approval.approved",
        "approval.denied",
        "approval.consumed",
        "decide_approval_with_proof",
        "approval_already_consumed",
        "approval_expired",
        "request_hash",
        "nonce",
    ] {
        assert!(
            approvals.contains(marker) || journal.contains(marker) || existing.contains(marker),
            "approval single-use marker missing: {marker}"
        );
    }
    assert!(journal.contains("append_expected"));
    assert!(journal.contains("ApprovalConsumptionFact"));
    assert!(existing.contains("approval_resumes_the_stored_request_once_with_monotonic_events"));
}

#[test]
fn expired_or_conflicting_decisions_fail_closed_without_reusing_execution_identity() {
    let approvals = include_str!("../src/approvals.rs");
    let journal = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    assert!(approvals.contains("approval_decision_conflict"));
    assert!(approvals.contains("event_request_id"));
    assert!(journal.contains("approval_expired"));
    assert!(journal.contains("approval_decision_conflict"));
    assert!(journal.contains("subject_request_id"));
}
