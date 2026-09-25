use kiana_core::{
    compare_notification_entrypoints, NotificationEntrypoint, NotificationEntrypointSnapshot,
    NotificationParityDisposition, NOTIFICATION_ENTRYPOINT_SNAPSHOT_SCHEMA,
};
use serde_json::Value;

fn snapshot(
    entrypoint: NotificationEntrypoint,
    fresh_process_rebuilt: bool,
) -> NotificationEntrypointSnapshot {
    let mut value = NotificationEntrypointSnapshot {
        schema: NOTIFICATION_ENTRYPOINT_SNAPSHOT_SCHEMA.to_owned(),
        entrypoint,
        instance_id: "instance-nm21".to_owned(),
        authority_epoch: "epoch-nm21".to_owned(),
        source_cursor: 21,
        notification_ids: vec!["notification:nm21".to_owned()],
        action_ids: vec!["action:nm21".to_owned()],
        terminal_result_ids: vec!["run:nm21".to_owned()],
        pending_ids: vec!["notification:nm21".to_owned()],
        delivery_attempt_ids: vec!["delivery:nm21".to_owned()],
        fresh_process_rebuilt,
        projection_digest: String::new(),
    };
    value.projection_digest = value.digest();
    value
}

fn all_snapshots() -> Vec<NotificationEntrypointSnapshot> {
    vec![
        snapshot(NotificationEntrypoint::CliTty, true),
        snapshot(NotificationEntrypoint::Web, true),
        snapshot(NotificationEntrypoint::Desktop, true),
        snapshot(NotificationEntrypoint::Workbench, true),
    ]
}

#[test]
fn four_entrypoints_with_one_source_projection_are_consistent() {
    let report = compare_notification_entrypoints(all_snapshots()).unwrap();
    assert_eq!(
        report.disposition,
        NotificationParityDisposition::Consistent
    );
    assert_eq!(report.source_cursor, 21);
    assert!(report.mismatches.is_empty());
    assert_eq!(report.next_action, "no_entrypoint_drift");
    report.validate().unwrap();
}

#[test]
fn source_or_fact_drift_is_unknown_and_never_completion() {
    let mut snapshots = all_snapshots();
    snapshots[2]
        .notification_ids
        .push("notification:foreign".to_owned());
    snapshots[2].notification_ids.sort();
    snapshots[2].projection_digest = snapshots[2].digest();
    let report = compare_notification_entrypoints(snapshots).unwrap();
    assert_eq!(report.disposition, NotificationParityDisposition::Unknown);
    assert!(report.next_action.contains("query_original"));
    assert!(!report.mismatches.is_empty());

    let mut missing_rebuild = all_snapshots();
    missing_rebuild[3].fresh_process_rebuilt = false;
    missing_rebuild[3].projection_digest = missing_rebuild[3].digest();
    let report = compare_notification_entrypoints(missing_rebuild).unwrap();
    assert_eq!(report.disposition, NotificationParityDisposition::Unknown);
    assert!(report
        .mismatches
        .iter()
        .any(|item| item == "fresh_process_rebuild_missing"));
}

#[test]
fn missing_or_duplicate_entrypoint_fails_closed() {
    let mut missing = all_snapshots();
    missing.pop();
    assert!(compare_notification_entrypoints(missing).is_err());
    let duplicate = vec![
        snapshot(NotificationEntrypoint::CliTty, true),
        snapshot(NotificationEntrypoint::CliTty, true),
        snapshot(NotificationEntrypoint::Desktop, true),
        snapshot(NotificationEntrypoint::Workbench, true),
    ];
    assert!(compare_notification_entrypoints(duplicate).is_err());
}

#[test]
fn fixture_captures_cross_entry_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/nm21-notification-parity.json"))
            .expect("valid NM-21 fixture");
    assert_eq!(fixture["schema"], "kiana.notification-entrypoint-parity.v1");
    assert_eq!(fixture["entrypoints"].as_array().unwrap().len(), 4);
    assert_eq!(fixture["consistent_next_action"], "no_entrypoint_drift");
    assert_eq!(
        fixture["unknown_next_action"],
        "query_original_source_and_rebuild_snapshot"
    );
}
