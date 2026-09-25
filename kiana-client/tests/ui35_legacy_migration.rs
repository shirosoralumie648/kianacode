use kiana_client::{
    map_legacy_route, validate_mapping, LegacyMigrationError, LegacySurface, MigrationDisposition,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[test]
fn fixture_declares_one_way_migration_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui35-legacy-migration.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.ui-legacy-migration-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("second")));
}

#[test]
fn known_routes_are_deprecated_typed_client_mappings() {
    for (surface, legacy, canonical) in [
        (LegacySurface::Web, "GET /api/state", "ui.snapshot"),
        (LegacySurface::Web, "GET /api/events", "ui.feed"),
        (LegacySurface::Cli, "run", "turn.prompt"),
        (LegacySurface::Cli, "cancel", "turn.cancel"),
        (LegacySurface::Cli, "receipt", "receipt.query"),
    ] {
        let mapping = map_legacy_route(surface, legacy).unwrap();
        assert_eq!(mapping.canonical_name.as_deref(), Some(canonical));
        assert_eq!(mapping.disposition, MigrationDisposition::Deprecated);
        assert!(!mapping.writes_facts);
        assert!(mapping.requires_typed_client);
        validate_mapping(&mapping).unwrap();
    }
}

#[test]
fn unknown_or_malicious_routes_fail_closed() {
    assert_eq!(
        map_legacy_route(LegacySurface::Web, "unknown"),
        Err(LegacyMigrationError::Unknown)
    );
    assert_eq!(
        map_legacy_route(LegacySurface::Web, "run\0 --token=secret"),
        Err(LegacyMigrationError::InputInvalid)
    );
}
