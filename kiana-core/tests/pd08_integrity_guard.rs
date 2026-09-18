#[test]
fn integrity_scan_reports_corrupt_unknown_and_health_gate_without_repairing_facts() {
    let integrity = include_str!("../../kiana-eventlog/src/integrity.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    for marker in [
        "IntegrityScanReport",
        "IntegrityScanStatus",
        "quarantine_required",
        "eventlog_integrity_quarantine_required",
        "eventlog_integrity_unknown",
        "scan_jsonl",
        "quarantine_and_reconcile",
        "pause_and_reconcile",
    ] {
        assert!(
            integrity.contains(marker),
            "integrity marker missing: {marker}"
        );
    }
    assert!(jsonl.contains("load_delta"));
    assert!(!integrity.contains("remove_file"));
    assert!(!integrity.contains("CapabilityBroker"));
    assert!(!integrity.contains("KianaHarness"));
}
