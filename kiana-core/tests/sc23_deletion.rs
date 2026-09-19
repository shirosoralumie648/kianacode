use kiana_core::plan_deletion;
use kiana_domain::{
    DataClass, DataPayloadState, DeleteRequest, EventId, LegalHold, LegalHoldReceipt, RequestId,
    RetentionDecision, RetentionDisposition, RetentionScan,
};
use std::collections::BTreeSet;

fn hash(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn scan(disposition: RetentionDisposition) -> RetentionScan {
    let event_id = EventId::new();
    let held = disposition == RetentionDisposition::Held;
    RetentionScan::new(
        "/repo",
        7,
        3,
        5,
        4,
        vec![event_id],
        vec![RetentionDecision {
            object_ref: "src/input.txt".to_owned(),
            class: DataClass::Restricted,
            purpose_id: "delete".to_owned(),
            source_digest: hash('a'),
            payload: DataPayloadState::Expired,
            disposition,
            retain_until_ms: 10,
            hold_id: held.then(|| "hold-1".to_owned()),
        }],
        if held {
            let hold = LegalHold::new(
                "hold-1",
                "/repo",
                BTreeSet::from(["src/input.txt".to_owned()]),
                "regulatory review",
                "principal:operator",
                10,
                7,
                true,
            )
            .unwrap();
            vec![LegalHoldReceipt::new(&hold, 5, 4, vec![event_id]).unwrap()]
        } else {
            Vec::new()
        },
    )
    .unwrap()
}

fn request() -> DeleteRequest {
    DeleteRequest::new(
        RequestId::new(),
        "/repo",
        BTreeSet::from(["src/input.txt".to_owned()]),
        "delete",
        "data_subject_request",
        "principal:operator",
        7,
        3,
        5,
    )
    .unwrap()
}

#[test]
fn eligible_delete_advances_epoch_and_starts_unknown_propagation() {
    let plan = plan_deletion(&request(), &scan(RetentionDisposition::Eligible)).unwrap();
    assert_eq!(plan.next_data_epoch, 4);
    assert_eq!(plan.tombstones.len(), 1);
    assert_eq!(plan.tombstones[0].data_epoch, 4);
    assert!(plan.manifest.has_unknown_propagation());
    plan.validate().unwrap();
}

#[test]
fn hold_or_unknown_never_becomes_a_delete_plan() {
    assert_eq!(
        plan_deletion(&request(), &scan(RetentionDisposition::Held)).unwrap_err(),
        "deletion_legal_hold_active"
    );
    assert_eq!(
        plan_deletion(&request(), &scan(RetentionDisposition::Unknown)).unwrap_err(),
        "deletion_retention_unknown"
    );
}

#[test]
fn stale_source_cursor_is_rejected_before_tombstone_creation() {
    let mut request = request();
    request.source_cursor = 4;
    request.request_digest = request.digest();
    assert_eq!(
        plan_deletion(&request, &scan(RetentionDisposition::Eligible)).unwrap_err(),
        "deletion_source_cursor_stale"
    );
}
