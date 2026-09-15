use kiana_core::{rebuild_audit_projection, AuditProjection, AuditProjectionError};
use kiana_domain::{AuditProjectionSnapshot, EventId, RequestId, RuntimeEvent};
use serde_json::json;

fn event(sequence: u64, kind: &str, source_cursor: u64) -> RuntimeEvent {
    let run_id = "11111111-1111-4111-8111-111111111111";
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        kind,
        json!({
            "run_id": run_id,
            "authority_epoch": 1,
            "data_epoch": 1,
            "source_cursor": source_cursor,
        }),
    )
    .unwrap()
}

#[test]
fn rebuild_is_deterministic_and_checkpoint_is_digest_bound() {
    let events = vec![event(1, "run.authorized", 1), event(2, "run.completed", 2)];
    let first = rebuild_audit_projection(&events, 1).unwrap();
    let second = rebuild_audit_projection(&events, 1).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.records.len(), 2);
    first.validate().unwrap();
    assert_eq!(
        first.checkpoint.records_digest,
        kiana_domain::json_digest(&serde_json::to_value(&first.records).unwrap())
    );
    let mut forged = first.clone();
    forged.checkpoint.records_digest =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
    assert!(forged.validate().is_err());
    let projection = AuditProjection::from_snapshot(first.clone()).unwrap();
    let restored =
        AuditProjection::restore(first.checkpoint.clone(), first.records.clone()).unwrap();
    assert_eq!(projection.snapshot(), restored.snapshot());
}

#[test]
fn incremental_page_requires_contiguous_cursor_and_preserves_original_facts() {
    let first_event = event(1, "run.authorized", 1);
    let second_event = event(2, "run.completed", 2);
    let mut projection = AuditProjection::rebuild(&[first_event.clone()], 1).unwrap();
    let original = projection.records().to_vec();
    projection
        .apply_page(std::slice::from_ref(&second_event), 2)
        .unwrap();
    assert_eq!(projection.records().len(), 2);
    assert_eq!(original.len(), 1);
    assert!(matches!(
        projection.apply_page(&[event(3, "run.failed", 4)], 4),
        Err(AuditProjectionError::SourceCursorGap)
    ));
    assert_eq!(projection.records().len(), 2);
}

#[test]
fn malformed_schema_decision_duplicate_and_cursor_gap_fail_closed() {
    let mut forged = event(1, "audit.forged", 1);
    assert!(rebuild_audit_projection(&[forged.clone()], 1).is_err());

    forged = event(1, "run.completed", 1);
    forged.data["decision"] = json!("denied");
    assert!(matches!(
        rebuild_audit_projection(&[forged], 1),
        Err(AuditProjectionError::ReduceFailed(_))
    ));

    let duplicate = event(1, "run.authorized", 1);
    let mut duplicate_id = duplicate.clone();
    duplicate_id.sequence = 2;
    assert!(matches!(
        rebuild_audit_projection(&[duplicate, duplicate_id], 1),
        Err(AuditProjectionError::SourceEventDuplicate)
    ));

    let gap = vec![
        event(1, "run.authorized", 10),
        event(2, "run.completed", 12),
    ];
    assert_eq!(
        rebuild_audit_projection(&gap, 10).unwrap_err(),
        AuditProjectionError::SourceCursorGap
    );
}

#[test]
fn snapshot_serde_round_trip_keeps_projection_contract_closed() {
    let snapshot: AuditProjectionSnapshot =
        rebuild_audit_projection(&[event(1, "run.authorized", 1)], 1).unwrap();
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert_eq!(
        serde_json::from_str::<AuditProjectionSnapshot>(&encoded).unwrap(),
        snapshot
    );
    let unknown = format!(
        "{}",
        encoded.trim_end_matches('}').to_owned() + ",\"unknown\":true}"
    );
    assert!(serde_json::from_str::<AuditProjectionSnapshot>(&unknown).is_err());
    assert_ne!(snapshot.source_event_ids, vec![EventId::new()]);
}
