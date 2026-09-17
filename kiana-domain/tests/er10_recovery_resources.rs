use kiana_domain::{
    json_digest, ApprovalId, BudgetLeaseId, CellId, EventId, RecoveryResourceSnapshot,
    SchemaVersion, StorageLockId, MAX_RECOVERY_RESOURCE_ENTRIES, RECOVERY_RESOURCE_SNAPSHOT_SCHEMA,
    RECOVERY_RESOURCE_SNAPSHOT_VERSION,
};
use serde_json::json;

#[test]
fn recovery_resource_snapshot_is_strict_and_canonical() {
    let approval = ApprovalId::new();
    let budget = BudgetLeaseId::new();
    let lease = StorageLockId::new();
    let cell = CellId::new();
    let source = EventId::new();
    let snapshot = RecoveryResourceSnapshot::new(
        9,
        vec![source, source],
        vec![approval, approval],
        vec![budget],
        vec![lease],
        vec![cell],
        vec![cell],
    )
    .unwrap();
    assert_eq!(snapshot.schema, RECOVERY_RESOURCE_SNAPSHOT_SCHEMA);
    assert_eq!(snapshot.version, RECOVERY_RESOURCE_SNAPSHOT_VERSION);
    assert_eq!(snapshot.source_event_ids, vec![source]);
    assert_eq!(snapshot.pending_approval_ids, vec![approval]);
    assert!(snapshot.validate().is_ok());
    assert_eq!(
        RecoveryResourceSnapshot::from_json(&snapshot.to_json().unwrap()).unwrap(),
        snapshot
    );
    assert_eq!(
        snapshot.snapshot_digest,
        json_digest(&json!({
            "schema": snapshot.schema,
            "version": snapshot.version,
            "source_cursor": snapshot.source_cursor,
            "source_event_ids": snapshot.source_event_ids,
            "pending_approval_ids": snapshot.pending_approval_ids,
            "reserved_budget_lease_ids": snapshot.reserved_budget_lease_ids,
            "active_resource_lease_ids": snapshot.active_resource_lease_ids,
            "active_cell_ids": snapshot.active_cell_ids,
            "fenced_cell_ids": snapshot.fenced_cell_ids,
        }))
    );
    assert!(MAX_RECOVERY_RESOURCE_ENTRIES >= 256);
}

#[test]
fn recovery_resource_snapshot_rejects_unknown_and_tampered_fields() {
    let snapshot = RecoveryResourceSnapshot::new(
        1,
        vec![EventId::new()],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    )
    .unwrap();
    let mut unknown = snapshot.to_json().unwrap();
    unknown["raw_budget"] = json!("do-not-infer");
    assert_eq!(
        RecoveryResourceSnapshot::from_json(&unknown).unwrap_err(),
        "recovery_resource_snapshot_decode_failed"
    );

    let mut tampered = snapshot;
    tampered.source_cursor = 2;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "recovery_resource_snapshot_digest_mismatch"
    );

    let mut version = tampered;
    version.version = SchemaVersion::new(2, 0);
    assert_eq!(
        version.validate().unwrap_err(),
        "recovery_resource_snapshot_header_invalid"
    );
}
