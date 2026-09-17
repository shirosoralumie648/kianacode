#[test]
fn cp10_approval_facts_use_one_journal_cas_and_keep_approval_separate_from_dispatch() {
    let journal = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let core = include_str!("../src/approvals.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/approval_journal.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    for marker in [
        "ApprovalDecisionFact",
        "ApprovalConsumptionFact",
        "decision_fact",
        "consumption_fact",
        "approval_expected_version_conflict",
        "prepare_consumption",
        "approval_atomic_consumption_required",
        "ApprovalState::Approved",
        "ApprovalState::Consumed",
    ] {
        assert!(
            journal.contains(marker)
                || core.contains(marker)
                || ports.contains(marker)
                || protocol.contains(marker)
                || domain.contains(marker)
                || daemon.contains(marker),
            "CP-10 marker missing: {marker}"
        );
    }
    assert!(ports.contains("decide_with_proof_and_version"));
    assert!(protocol.contains("expected_version"));
    assert!(core.contains("replay_approval_decision"));
    assert!(daemon.contains("JournalApprovalStore::new"));
    assert!(!daemon.contains("MemoryApprovalStore::new"));
    for forbidden in ["standing_approval", "authorize_and_execute_from_preview"] {
        assert!(
            !journal.contains(forbidden) && !core.contains(forbidden),
            "CP-10 must not enable {forbidden}"
        );
    }
}
