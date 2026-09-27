#[test]
fn restore_verification_keeps_fences_epochs_and_pause_visible() {
    let domain = include_str!("../../kiana-domain/src/restore_verification.rs");
    let core = include_str!("../src/restore_verification.rs");
    for marker in [
        "RestoreVerificationFact",
        "snapshot_manifest_digest",
        "projection_cursor",
        "old_instance_fenced",
        "old_authority_fenced",
        "pending_approval_count",
        "unknown_effect_count",
        "default_paused",
        "validate_restore_verification_fact",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "PD-23 marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStore::append",
        "CapabilityBroker::new",
        "std::process::Command",
        "activate_root",
        "revive_trigger",
        "auto_success",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "PD-23 Core/domain executes restore effects: {forbidden}"
        );
    }
}
