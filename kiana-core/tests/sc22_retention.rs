use kiana_core::scan_retention;
use kiana_domain::{
    DataClass, DataGovernanceSnapshot, DataPayloadState, DataRetentionObservation, EventId,
    LegalHold, RetentionDisposition, RetentionPolicy,
};
use std::collections::{BTreeMap, BTreeSet};

fn hash(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn policy() -> RetentionPolicy {
    RetentionPolicy::new(
        "/repo",
        7,
        3,
        100,
        BTreeMap::from([("src/held.txt".to_owned(), 10)]),
    )
    .unwrap()
}

fn snapshot(payload: DataPayloadState) -> DataGovernanceSnapshot {
    DataGovernanceSnapshot::new(
        "/repo",
        7,
        3,
        5,
        vec![EventId::new()],
        vec![kiana_domain::DataRetentionObservation {
            source_ref: "src/held.txt".to_owned(),
            class: DataClass::Restricted,
            purpose_id: "audit".to_owned(),
            payload,
            audit_metadata_retained: true,
            source_digest: hash('a'),
        }],
        BTreeMap::from([("audit".to_owned(), payload)]),
        true,
    )
    .unwrap()
}

#[test]
fn legal_hold_is_scanned_before_expiry_and_has_a_cursor_bound_receipt() {
    let hold = LegalHold::new(
        "hold-1",
        "/repo",
        BTreeSet::from(["src/held.txt".to_owned()]),
        "regulatory review",
        "principal:operator",
        10,
        7,
        true,
    )
    .unwrap();
    let scan = scan_retention(
        &policy(),
        &[hold],
        &snapshot(DataPayloadState::Available),
        4,
        1000,
    )
    .unwrap();
    assert_eq!(scan.decisions[0].disposition, RetentionDisposition::Held);
    assert_eq!(scan.hold_receipts.len(), 1);
    assert_eq!(scan.hold_receipts[0].source_cursor, scan.source_cursor);
    assert_eq!(
        scan.hold_receipts[0].projection_cursor,
        scan.projection_cursor
    );
    scan.validate().unwrap();
}

#[test]
fn unknown_source_is_not_treated_as_purgeable() {
    let scan = scan_retention(
        &policy(),
        &[],
        &snapshot(DataPayloadState::Unknown),
        4,
        1000,
    )
    .unwrap();
    assert_eq!(scan.decisions[0].disposition, RetentionDisposition::Unknown);
}

#[test]
fn policy_revision_drift_is_rejected_before_scan() {
    let mut stale = snapshot(DataPayloadState::Available);
    stale.policy_revision = 6;
    stale.snapshot_digest = stale.digest();
    assert_eq!(
        scan_retention(&policy(), &[], &stale, 4, 1000).unwrap_err(),
        "retention_policy_revision_stale"
    );
}
