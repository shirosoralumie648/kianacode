use kiana_core::{
    notification_fault_matrix, NotificationFaultDisposition, NotificationFaultScenario,
    NOTIFICATION_FAULT_MATRIX_SCHEMA,
};
use serde_json::Value;

#[test]
fn deterministic_matrix_covers_notification_faults_and_preserves_critical_facts() {
    let matrix = notification_fault_matrix(
        20,
        20,
        vec![
            "event-b".to_owned(),
            "event-a".to_owned(),
            "event-a".to_owned(),
        ],
    )
    .unwrap();
    assert_eq!(matrix.schema, NOTIFICATION_FAULT_MATRIX_SCHEMA);
    assert_eq!(matrix.source_event_ids, vec!["event-a", "event-b"]);
    assert_eq!(matrix.cases.len(), 9);
    assert!(matrix.cases.iter().all(|case| case.critical_preserved));
    assert!(matrix.cases.iter().all(|case| !case.effect_started));
    assert!(matrix.cases.iter().all(|case| case.secret_free));
    assert!(matrix
        .cases
        .iter()
        .any(|case| case.scenario == NotificationFaultScenario::Duplicate
            && case.duplicate_suppressed));
    assert!(matrix
        .cases
        .iter()
        .any(|case| case.scenario == NotificationFaultScenario::CursorGap
            && case.disposition == NotificationFaultDisposition::SnapshotRequired));
    matrix.validate().unwrap();
}

#[test]
fn fault_matrix_rejects_empty_or_zero_seed_inputs() {
    assert!(notification_fault_matrix(0, 20, vec!["event".to_owned()]).is_err());
    assert!(notification_fault_matrix(20, 0, vec!["event".to_owned()]).is_err());
    assert!(notification_fault_matrix(20, 20, Vec::new()).is_err());
}

#[test]
fn fixture_captures_bounded_notification_fault_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/nm20-notification-faults.json"))
            .expect("valid NM-20 fixture");
    assert_eq!(fixture["schema"], "kiana.notification-fault-matrix.v1");
    assert_eq!(fixture["scenarios"].as_array().unwrap().len(), 9);
    for invariant in ["critical_preserved", "no_effect_started", "secret_free"] {
        assert!(fixture["invariants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == invariant));
    }
}
