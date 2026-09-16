#[test]
fn schema_and_event_registry_are_the_single_compatibility_boundary() {
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let events = include_str!("../../kiana-domain/src/event_contracts.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "SCHEMA_CONTRACTS",
        "check_schema_compatibility",
        "allow_unknown_fields",
        "SchemaLayer::Wire",
        "SchemaLayer::RuntimeEvent",
    ] {
        assert!(
            contracts.contains(marker),
            "schema registry marker missing: {marker}"
        );
    }
    for marker in [
        "event_kind_spec",
        "event_kind_is_required",
        "event_migration",
        "unknown_required_event_kind",
    ] {
        assert!(
            events.contains(marker),
            "event registry marker missing: {marker}"
        );
    }
    assert!(protocol.contains("check_schema_compatibility"));
}
