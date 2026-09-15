#[test]
fn event_contract_registry_and_migration_boundary_are_source_owned() {
    let contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    let domain_contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let states = include_str!("../../kiana-domain/src/states.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/event-receipt-schema-baseline.md");

    for marker in [
        "EventKindSpec",
        "EVENT_KIND_SPECS",
        "EVENT_MIGRATIONS",
        "unknown_required_event_kind",
        "event_schema_version_incompatible",
        "event_payload_unknown_field",
        "validate_runtime_event",
        "secret_policy",
        "required_ids",
    ] {
        assert!(
            contracts.contains(marker),
            "missing event contract marker {marker}"
        );
    }
    assert!(domain_contracts.contains("kiana.runtime-event.v1"));
    assert!(states.contains("pub struct RuntimeEvent"));
    assert!(states.contains("pub event_id: EventId"));
    assert!(journal.contains("JOURNAL_FRAME_SCHEMA"));
    assert!(protocol.contains("RuntimeEvent") || protocol.contains("EventKind"));
    assert!(baseline.contains("unknown_required_event_kind_fails_closed"));
    assert!(baseline.contains("event_schema_version_cannot_downgrade"));
    assert!(baseline.contains("event_payload_unknown_field_is_not_silently_dropped"));
    assert!(baseline.contains("legacy decode"));
}
