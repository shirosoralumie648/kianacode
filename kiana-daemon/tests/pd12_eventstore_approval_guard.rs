//! PD-12 source guard: product approval authority is the shared EventStore journal.

#[test]
fn daemon_wires_journal_approval_store_and_not_legacy_memory_authority() {
    let daemon = include_str!("../src/lib.rs");
    let journal = include_str!("../src/journal_approvals.rs");
    let compatibility = include_str!("../src/approval_store.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    for marker in [
        "JournalApprovalStore::new",
        "commit_transition",
        "prepare_activation",
        "prepare_consumption",
        "approval_legacy_reauthorization_required",
    ] {
        assert!(
            daemon.contains(marker) || journal.contains(marker) || ports.contains(marker),
            "PD-12 EventStore approval marker missing: {marker}"
        );
    }
    assert!(!daemon.contains("MemoryApprovalStore::new"));
    assert!(compatibility.contains("MemoryApprovalStore"));
    assert!(
        compatibility.contains("legacy")
            || compatibility.contains("compat")
            || compatibility.contains("JSONL")
    );
    for forbidden in ["authorize_and_execute", "CapabilityBrokerPort::execute"] {
        assert!(
            !journal.contains(forbidden),
            "approval journal bypasses authority: {forbidden}"
        );
    }
}
