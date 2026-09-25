#[test]
fn notification_projector_is_rebuildable_and_append_only() {
    let source = include_str!("../src/notification_projector.rs");
    let domain = include_str!("../../kiana-domain/src/notification_lifecycle.rs");
    for marker in [
        "NotificationProjector",
        "rebuild_from_committed",
        "apply_committed",
        "NotificationLifecycleFact",
        "NotificationVisibility",
        "source_cursor",
        "data_epoch",
        "TerminalRewrite",
        "FactConflict",
    ] {
        assert!(
            source.contains(marker) || domain.contains(marker),
            "NM-12 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "std::fs",
        "remove_file",
        "HumanTaskStatus::",
        "EventStorePort::append",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-12 boundary widened: {forbidden}"
        );
    }
}
