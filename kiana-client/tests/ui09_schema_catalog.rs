use kiana_client::{schema_contract, UiSchemaContract, UI_SCHEMA_CONTRACTS, UI_SCHEMA_LOCK_SCHEMA};

#[test]
fn generated_catalog_is_sorted_bounded_and_strict() {
    assert_eq!(UI_SCHEMA_LOCK_SCHEMA, "kiana.ui-schema-lock.v1");
    assert!(!UI_SCHEMA_CONTRACTS.is_empty());
    for pair in UI_SCHEMA_CONTRACTS.windows(2) {
        assert!(pair[0].id < pair[1].id, "generated ids must be sorted");
    }
    for contract in UI_SCHEMA_CONTRACTS {
        assert!(contract.max_bytes > 0);
        assert_eq!(contract.unknown_fields, "deny");
        assert_eq!(schema_contract(contract.id), Some(contract));
    }
}

#[test]
fn generated_catalog_contains_the_ui07_and_ui08_boundaries() {
    let expected = [
        ("kiana.ui-handshake-request.v1", "UiHandshakeRequest"),
        ("kiana.ui-handshake-response.v1", "UiHandshakeResponse"),
        ("kiana.ui-feed-frame.v1", "UiFeedFrameV1"),
        ("kiana.ui-action.v1", "UiActionV1"),
        ("kiana.ui-action-result.v1", "UiActionResult"),
        ("kiana.ui-entity-store-snapshot.v1", "UiEntityStoreSnapshot"),
        ("kiana.ui-snapshot.v1", "UiSnapshotV1"),
    ];
    for (id, rust_type) in expected {
        let contract: &UiSchemaContract = schema_contract(id).expect("schema must be locked");
        assert_eq!(contract.rust_type, rust_type);
    }
}
