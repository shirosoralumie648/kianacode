#[test]
fn dep07_storage_preflight_is_metadata_only_and_fail_closed() {
    let domain = include_str!("../../kiana-domain/src/storage_preflight.rs");
    let core = include_str!("../src/storage_preflight.rs");
    for marker in [
        "StoragePreflightRequest",
        "StoragePreflightReport",
        "ProjectTrustSnapshot",
        "StorageFileIdentity",
        "symlink_target",
        "hardlink_target",
        "remote_filesystem_unsupported",
        "capacity_bytes_insufficient",
        "evaluate_storage_preflight",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-07 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "dispatch_capability",
        "symlink_metadata",
        "create_dir",
        "acquire_lock",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-07 preflight crossed an effect boundary: {forbidden}"
        );
    }
}
