#[test]
fn storage_root_is_resolved_once_and_lock_adapter_stays_outside_control_plane() {
    let domain = include_str!("../../kiana-domain/src/storage.rs");
    let daemon = include_str!("../../kiana-daemon/src/storage.rs");
    let local_io = include_str!("../../kiana-daemon/src/local_packages.rs");
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
        "STORAGE_LOCK_SCHEMA",
        "store_identity_header_invalid",
        "store_identity_time_invalid",
        "storage_lock_time_invalid",
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
        "storage_lock_corrupt",
        "store-identity.json",
        "create_new_file",
        "/proc/mounts",
    ] {
        assert!(
            daemon.contains(marker),
            "storage daemon marker missing: {marker}"
        );
    }
    assert!(host.contains("pub fn storage_root"));
    assert!(host.contains("pub fn acquire_storage"));
    for marker in [
        "libc::O_EXCL",
        "libc::O_NOFOLLOW",
        "libc::O_NONBLOCK",
        "libc::openat",
        "libc::unlinkat",
        "metadata.is_file()",
        "read_optional",
        "remove_owned_file",
    ] {
        assert!(
            local_io.contains(marker),
            "storage I/O marker missing: {marker}"
        );
    }
    // Module documentation describes the execution boundary. Check executable source so
    // documenting that boundary cannot be mistaken for depending on the execution layer.
    for source in [daemon, local_io] {
        let code = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!code.contains("ControlPlane"));
        assert!(!code.contains("CapabilityBroker"));
        assert!(!code.contains("dispatch_capability"));
    }
}
