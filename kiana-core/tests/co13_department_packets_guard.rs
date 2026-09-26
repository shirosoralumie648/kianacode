#[test]
fn department_packets_are_versioned_and_cannot_grant_runtime_authority() {
    let packets = include_str!("../../kiana-domain/src/department_packets.rs");
    let legacy = include_str!("../../kiana-domain/src/work_packets.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "DEPARTMENT_PACKET_SCHEMA",
        "RESULT_CONTRACT_SCHEMA",
        "DepartmentPacketKind",
        "PacketInputBasis",
        "ResultContract",
        "runtime_grant",
        "budget_lease",
        "department_packet_runtime_authority_forbidden",
        "department_packet_plan_required",
        "department_packet_write_scope_denied",
        "department_packet_role_department_mismatch",
        "packet_role_must_be_builder",
        "input_basis",
        "output_schema",
    ] {
        assert!(
            packets.contains(marker) || legacy.contains(marker) || core.contains(marker),
            "CO-13 marker missing: {marker}"
        );
    }
    assert!(legacy.contains("pub fn validate"));
    assert!(legacy.contains("packet_role_must_be_builder"));
    assert!(!packets.contains("CapabilityBroker"));
    assert!(!packets.contains("EventStorePort"));
    assert!(!core.contains("DepartmentPacket::execute"));
}
