use kiana_core::{fault_matrix, fault_matrix_from_events, replay_fault_matrix};
use kiana_domain::{
    EventId, FaultCaseStatus, FaultInjectionPoint, FaultMatrix, RequestId, RuntimeEvent,
};
use serde_json::json;

fn event(sequence: u64) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        "run.accepted",
        json!({"source_cursor": sequence}),
    )
    .unwrap()
}

#[test]
fn every_fault_boundary_is_replayable_and_preserves_safety_invariants() {
    let ids = vec![EventId::new(), EventId::new()];
    let matrix = fault_matrix(7, 2, ids.clone()).unwrap();
    let replay = replay_fault_matrix(7, 2, ids).unwrap();
    assert_eq!(matrix, replay);
    assert_eq!(matrix.cases.len(), 8);
    assert_eq!(
        matrix
            .cases
            .iter()
            .filter(|case| case.status == FaultCaseStatus::Rejected)
            .count(),
        1
    );
    assert!(matrix.cases.iter().all(|case| {
        !case.duplicate_effect
            && !case.false_success
            && (case.status != FaultCaseStatus::Unknown || case.resource_fenced)
    }));
    assert!(matrix
        .cases
        .iter()
        .any(|case| case.point == FaultInjectionPoint::Result && !case.effect_known));
    matrix.validate().unwrap();
}

#[test]
fn event_seed_is_bounded_and_duplicate_source_is_rejected() {
    let events = vec![event(1), event(2)];
    let matrix = fault_matrix_from_events(9, &events).unwrap();
    assert_eq!(matrix.source_cursor, 2);
    matrix.validate().unwrap();
    let duplicate = events[0].clone();
    assert!(fault_matrix_from_events(9, &[events[0].clone(), duplicate]).is_err());
    assert!(fault_matrix(0, 1, vec![EventId::new()]).is_err());
    assert!(fault_matrix(1, 0, vec![EventId::new()]).is_err());
}

#[test]
fn forged_false_success_or_unfenced_unknown_is_rejected() {
    let id = EventId::new();
    let mut matrix = fault_matrix(11, 1, vec![id]).unwrap();
    matrix.cases[0].false_success = true;
    matrix.cases[0].case_digest = matrix.cases[0].digest();
    assert_eq!(
        matrix.validate().unwrap_err(),
        "fault_safety_invariant_failed"
    );

    let mut matrix = fault_matrix(12, 1, vec![EventId::new()]).unwrap();
    let unknown = matrix
        .cases
        .iter_mut()
        .find(|case| case.status == FaultCaseStatus::Unknown)
        .unwrap();
    unknown.resource_fenced = false;
    unknown.case_digest = unknown.digest();
    matrix.matrix_digest = matrix.digest();
    assert_eq!(matrix.validate().unwrap_err(), "fault_unknown_not_fenced");
}

#[test]
fn matrix_serde_is_closed() {
    let matrix: FaultMatrix = fault_matrix(3, 1, vec![EventId::new()]).unwrap();
    let mut encoded = serde_json::to_value(&matrix).unwrap();
    assert_eq!(
        serde_json::from_value::<FaultMatrix>(encoded.clone()).unwrap(),
        matrix
    );
    encoded["untrusted"] = json!(true);
    assert!(serde_json::from_value::<FaultMatrix>(encoded).is_err());
}
