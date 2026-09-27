#[test]
fn dep07_storage_adapter_rejects_symlink_components_before_canonicalize() {
    let source = include_str!("../src/storage.rs");
    let project_check = source
        .find("reject_symlink_components(project_root")
        .expect("project root symlink guard missing");
    let project_canonicalize = source
        .find("fs::canonicalize(project_root)")
        .expect("project root canonicalization missing");
    assert!(project_check < project_canonicalize);
    let root_check = source
        .find("reject_symlink_components(&configured")
        .expect("configured root symlink guard missing");
    let root_canonicalize = source
        .find("canonicalize_nonexistent(&configured)")
        .expect("configured root canonicalization missing");
    assert!(root_check < root_canonicalize);
    for marker in [
        "reject_symlink_components",
        "symlink_metadata",
        "storage_project_root_symlink",
        "storage_root_symlink",
    ] {
        assert!(
            source.contains(marker),
            "DEP-07 adapter marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "EventStore::append",
        "dispatch_capability",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-07 storage adapter crossed execution boundary: {forbidden}"
        );
    }
}
