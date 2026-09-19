#[test]
fn sc22_core_keeps_retention_scan_pure_and_hold_bound() {
    let source = include_str!("../src/retention.rs");
    for marker in [
        "policy.validate()",
        "snapshot.validate()",
        "retention_policy_revision_stale",
        "retention_data_epoch_stale",
        "LegalHoldReceipt::new",
        "RetentionDisposition::Unknown",
        "RetentionScan::new",
    ] {
        assert!(
            source.contains(marker),
            "missing SC-22 core marker: {marker}"
        );
    }
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("remove_file"));
}
