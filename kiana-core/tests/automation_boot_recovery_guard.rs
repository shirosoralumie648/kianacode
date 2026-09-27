#[test]
fn automation_boot_recovery_keeps_unknown_and_cursor_visible() {
    let domain = include_str!("../../kiana-domain/src/automation_boot_recovery.rs");
    let core = include_str!("../src/automation_boot_recovery.rs");
    for marker in [
        "AutomationBootRecoveryFact",
        "source_cursor",
        "projection_cursor",
        "pending_unknown_count",
        "ReconcileRequired",
        "validate_boot_recovery",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-21 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "auto_success",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-21 boundary executes effects: {forbidden}"
        );
    }
}
