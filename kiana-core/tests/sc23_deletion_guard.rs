#[test]
fn sc23_core_keeps_delete_deny_paths_and_no_direct_effect() {
    let source = include_str!("../src/deletion.rs");
    for marker in [
        "deletion_legal_hold_active",
        "deletion_retention_unknown",
        "deletion_source_cursor_stale",
        "checked_add(1)",
        "DeletionTombstone::new",
        "adapter_receipt_required",
    ] {
        assert!(
            source.contains(marker),
            "missing SC-23 core marker: {marker}"
        );
    }
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("remove_file"));
}
