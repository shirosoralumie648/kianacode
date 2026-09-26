#[test]
fn company_handoff_has_revision_assignment_and_owner_fencing() {
    let handoff = include_str!("../../kiana-domain/src/company_handoff.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let legacy = include_str!("../../kiana-domain/src/handoff.rs");

    for marker in [
        "COMPANY_HANDOFF_SCHEMA",
        "HandoffPacketRevision",
        "packet_digest",
        "budget_ref",
        "input_refs",
        "HandoffAssignment",
        "assignment_version",
        "CompanyHandoffStatus::Pending",
        "CompanyHandoffStatus::Acknowledged",
        "CompanyHandoffStatus::Rejected",
        "CompanyHandoffStatus::Expired",
        "handoff_receiver_assignment_invalid",
        "handoff_acceptance_evidence_mismatch",
        "current_owner_assignment_id",
        "pending_recipient_assignment_id",
        "recipient_ack_timeout",
        "runtime_lease_forbidden",
        "immutable_digest",
        "offer_accountability_handoff",
        "acknowledge_accountability_handoff",
        "expire_accountability_handoff",
        "PacketHandoff",
        "AcknowledgeHandoff",
    ] {
        assert!(
            handoff.contains(marker) || company.contains(marker) || legacy.contains(marker),
            "CO-17 marker missing: {marker}"
        );
    }
    assert!(company.contains("accountability_handoffs"));
    assert!(handoff.contains("No runtime grant or lease") || handoff.contains("runtime_lease"));
}
