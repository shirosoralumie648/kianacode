#[test]
fn quality_protocol_is_versioned_and_does_not_bypass_control_plane() {
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let registry = include_str!("../../kiana-domain/src/event_contracts.rs");
    let commands = include_str!("../src/commands.rs");
    for marker in [
        "QUALITY_COMMAND_SCHEMA",
        "QualityCommandKind",
        "QualityCommandRequest",
        "QUALITY_COMMAND_KINDS",
        "QUALITY_EVENT_KINDS",
        "quality_command",
        "quality_command_schema_invalid",
    ] {
        assert!(
            protocol.contains(marker),
            "protocol marker missing: {marker}"
        );
    }
    for marker in [
        "QUALITY_FIELDS",
        "eval.run",
        "eval.capture",
        "eval.compare",
        "quality.feedback",
        "quality.promote",
        "quality.rollback",
        "\"eval.\"",
        "\"quality.\"",
    ] {
        assert!(
            registry.contains(marker),
            "registry marker missing: {marker}"
        );
    }
    assert!(commands.contains("CommandIntent"));
    assert!(!protocol.contains("tokio::spawn"));
    assert!(!protocol.contains("CapabilityBroker"));
}
