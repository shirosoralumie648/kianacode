use kiana_domain::{
    check_schema_compatibility, event_kind_is_required, event_kind_spec, event_migration,
    schema_contract, SchemaLayer, SchemaVersion, SCHEMA_CONTRACTS,
};

#[test]
fn schema_registry_separates_layers_and_fails_closed_on_unknown_major() {
    let protocol = schema_contract("kiana.protocol.v1").unwrap();
    let company = schema_contract("kiana.company-event.v1").unwrap();
    assert_eq!(protocol.layer, SchemaLayer::Wire);
    assert_eq!(company.layer, SchemaLayer::RuntimeEvent);
    assert!(protocol.allow_unknown_fields);
    assert!(!company.allow_unknown_fields);
    assert!(check_schema_compatibility("kiana.protocol.v1", &SchemaVersion::new(1, 7)).is_ok());
    assert!(check_schema_compatibility("kiana.protocol.v1", &SchemaVersion::new(2, 0)).is_err());
    assert!(check_schema_compatibility("kiana.unknown.v1", &SchemaVersion::new(1, 0)).is_err());
}

#[test]
fn event_unknown_and_migration_rules_are_explicit() {
    assert!(event_kind_spec("run.authorized").is_some());
    assert!(event_kind_is_required("run.authorized"));
    assert!(event_kind_spec("future.opaque").is_none());
    assert!(event_migration("run.authorized", 1, 1).is_none());
    let names = SCHEMA_CONTRACTS
        .iter()
        .map(|contract| contract.name)
        .collect::<Vec<_>>();
    assert!(names.contains(&"kiana.company-command-receipt.v1"));
    assert!(names.contains(&"kiana.company-event.v1"));
}
