use kiana_core::NotificationProjection;
use kiana_domain::{MessageId, Notification, NotificationChannel, RequestId, RuntimeEvent};
use serde_json::json;

fn event(sequence: u64, source: &str) -> RuntimeEvent {
    let notification = Notification::new(
        MessageId::new(),
        "actor-1",
        None,
        vec!["global".to_owned()],
        NotificationChannel::InApp,
        1,
        10_000,
        1,
        None,
    )
    .unwrap();
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        "run.completed",
        json!({
            "source": source,
            "owner_id": "actor-1",
            "notification": notification,
        }),
    )
    .unwrap()
}

#[test]
fn notification_projector_consumes_committed_events_and_checkpoint() {
    let first = event(1, "eventlog");
    let mut projector = NotificationProjection::new();
    projector
        .apply_committed(std::slice::from_ref(&first), 1)
        .unwrap();
    assert_eq!(projector.notifications().len(), 1);
    assert_eq!(projector.source_cursor(), 1);
    let checkpoint = projector.checkpoint().unwrap().clone();
    projector
        .apply_committed(std::slice::from_ref(&first), 1)
        .unwrap();
    assert_eq!(projector.notifications().len(), 1);
    assert_eq!(projector.checkpoint(), Some(&checkpoint));
}

#[test]
fn notification_projector_rejects_untrusted_source_and_cursor_gap_without_mutation() {
    let first = event(1, "eventlog");
    let mut projector = NotificationProjection::new();
    projector
        .apply_committed(std::slice::from_ref(&first), 1)
        .unwrap();
    let before = projector.clone();
    assert!(projector.apply_committed(&[event(2, "model")], 2).is_err());
    assert_eq!(projector, before);
    assert!(projector.apply_committed(&[], 4).is_err());
    assert_eq!(projector, before);
}
