//! Guard for PD-08: the read-only integrity scan must classify stores, report recovered tails and
//! fail closed, without deleting/moving facts or opening a second execution path.

#[test]
fn integrity_scan_reports_corrupt_unknown_and_health_gate_without_repairing_facts() {
    let integrity = include_str!("../../kiana-eventlog/src/integrity.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let daemon_gate = include_str!("../../kiana-daemon/src/integrity_gate.rs");

    for marker in [
        "IntegrityScanReport",
        "IntegrityScanStatus",
        "IntegrityRecoveryReport",
        "Recovered",
        "repaired_tail",
        "quarantine_required",
        "eventlog_integrity_quarantine_required",
        "eventlog_integrity_unknown",
        "scan_jsonl",
        "health_gate",
        "quarantine_and_reconcile",
        "pause_and_reconcile",
        "repair_tail_and_continue",
        "open_with_recovery_async",
    ] {
        assert!(
            integrity.contains(marker),
            "integrity marker missing: {marker}"
        );
    }

    // The loader stays the single authority for framing/tail handling: the scan only consumes the
    // recovery it reports instead of inventing a second parser.
    for marker in [
        "load_delta",
        "eventlog_required_record_unsupported",
        "JournalRecovery",
        "repaired_tail",
        "open_with_recovery",
    ] {
        assert!(jsonl.contains(marker), "jsonl marker missing: {marker}");
    }

    // The daemon gate is read-only admission: it delegates to scan_jsonl and refuses execution
    // when the store is not healthy.
    for marker in [
        "StorageIntegrityGate",
        "StorageIntegrityAdmission",
        "admit_execution",
        "inspect",
        "scan_jsonl",
    ] {
        assert!(
            daemon_gate.contains(marker),
            "daemon gate marker missing: {marker}"
        );
    }

    assert!(!integrity.contains("remove_file"));
    assert!(!integrity.contains("CapabilityBroker"));
    assert!(!integrity.contains("KianaHarness"));
    assert!(!daemon_gate.contains("remove_file"));
    assert!(!daemon_gate.contains("set_len"));
    assert!(!daemon_gate.contains("CapabilityBroker"));
    assert!(!daemon_gate.contains("KianaHarness"));
}
