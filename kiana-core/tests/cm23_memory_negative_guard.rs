#[test]
fn memory_negative_gates_keep_server_owned_lifecycle() {
    let gate = include_str!("../../kiana-domain/src/memory_negative_gate.rs");
    let memory = include_str!("../../kiana-domain/src/memory.rs");
    for marker in [
        "reject_model_overrides",
        "memory_user_private_requires_operator",
        "MemoryOrigin::Model",
        "MemoryAdmission::Candidate",
        "MemoryAdmission::Ephemeral",
        "operator_approval_required",
        "visible_in_session",
    ] {
        assert!(
            gate.contains(marker) || memory.contains(marker),
            "CM-23 source marker missing: {marker}"
        );
    }
    assert!(!gate.contains("ModelClient"));
    assert!(!gate.contains("CapabilityBroker"));
}
