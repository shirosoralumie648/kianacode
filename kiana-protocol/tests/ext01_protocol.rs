use kiana_protocol::{
    ExtensionError, ExtensionErrorCode, ExtensionSnapshotState, EXTENSION_ERROR_SCHEMA,
    EXTENSION_SNAPSHOT_SCHEMA,
};

#[test]
fn protocol_reexports_versioned_extension_contracts() {
    let error = ExtensionError::new(ExtensionErrorCode::ScopeDenied, "scope denied", false);
    let encoded = serde_json::to_value(&error).unwrap();
    let decoded: ExtensionError = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.schema, EXTENSION_ERROR_SCHEMA);
    assert_eq!(EXTENSION_SNAPSHOT_SCHEMA, "kiana.extension-snapshot.v1");
    assert!(matches!(
        ExtensionSnapshotState::Prepared,
        ExtensionSnapshotState::Prepared
    ));
}
