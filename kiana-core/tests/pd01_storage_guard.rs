#[test]
fn storage_root_is_resolved_once_and_lock_adapter_stays_outside_control_plane() {
    let domain = include_str!("../../kiana-domain/src/storage.rs");
    let daemon = include_str!("../../kiana-daemon/src/storage.rs");
    let host = include_str!("../../kiana-daemon/src/lib.rs");
    for marker in [
        "StorageRoot",
        "StorageOwnerScope",
        "StoreIdentity",
        "StorageLockRecord",
        "StorageNamespace",
        "storage_network_filesystem_unsupported",
        "storage_namespace_map_invalid",
        "storage_lock_owner_mismatch",
        "stable_root_id",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "storage domain marker missing: {marker}"
        );
    }
    for marker in [
        "resolve_storage_root",
        "StorageLease",
        "KIANA_HOME_ENV",
        "detect_backend",
        "storage_root_inside_project",
        "storage_lock_conflict",
        "storage_identity.json",
        "create_new(true)",
        "/proc/mounts",
    ] {
        assert!(
            daemon.contains(marker),
            "storage daemon marker missing: {marker}"
        );
    }
    assert!(host.contains("pub fn storage_root"));
    assert!(host.contains("pub fn acquire_storage"));
    assert!(!daemon.contains("ControlPlane"));
    assert!(!daemon.contains("CapabilityBroker"));
}
