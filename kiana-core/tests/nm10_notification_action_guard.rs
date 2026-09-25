#[test]
fn notification_action_gate_is_control_plane_admission_only() {
    let gate = include_str!("../src/notification_action.rs");
    let command = include_str!("../../kiana-domain/src/notification_actions.rs");
    for marker in [
        "NotificationActionGate",
        "NotificationActionAdmission",
        "authoritative_ref",
        "RecipientMismatch",
        "RevisionConflict",
        "DigestConflict",
        "AuthorityConflict",
        "CursorConflict",
        "Expired",
        "control_plane_required",
        "direct_effect",
    ] {
        assert!(
            gate.contains(marker) || command.contains(marker),
            "NM-10 marker missing: {marker}"
        );
    }
    for source in [gate, command] {
        for forbidden in [
            "CapabilityBroker",
            "EventStorePort",
            "tokio::spawn",
            "KianaHarness",
            "HumanTaskStatus::Decided",
            ".append(",
            "execute(",
        ] {
            assert!(
                !source.contains(forbidden),
                "NM-10 direct effect marker: {forbidden}"
            );
        }
    }
}
