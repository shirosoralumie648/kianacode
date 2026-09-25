use kiana_core::{NotificationProjector, NotificationProjectorError, NotificationVisibility};
use kiana_domain::{
    EventId, NotificationId, NotificationLifecycleFact, NotificationLifecycleKind, RequestId,
    RuntimeEvent,
};
use serde_json::json;

fn event() -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        1,
        "run.started",
        json!({"source":"runtime"}),
    )
    .unwrap()
}

#[test]
fn rebuild_and_lifecycle_replay_are_cursor_and_epoch_bound() {
    let notification = NotificationId::new();
    let first = NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        1,
        2,
        NotificationLifecycleKind::Withdraw,
        None,
        "user withdrew obsolete notification",
    )
    .unwrap();
    let committed = event();
    let mut projector =
        NotificationProjector::rebuild_from_committed(&[committed.clone()], 1, &[first.clone()])
            .unwrap();
    assert_eq!(
        projector.visibility(notification),
        NotificationVisibility::Withdrawn
    );
    assert_eq!(projector.source_cursor(), 1);
    assert_eq!(projector.data_epoch(), 2);

    projector
        .apply_committed(&[committed], 1, &[first])
        .expect("exact replay is idempotent");
    let conflicting = NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        2,
        2,
        NotificationLifecycleKind::Expire,
        None,
        "late rewrite",
    )
    .unwrap();
    assert_eq!(
        projector.apply_committed(&[event()], 2, &[conflicting]),
        Err(NotificationProjectorError::TerminalRewrite)
    );
    assert_eq!(
        projector.visibility(notification),
        NotificationVisibility::Withdrawn
    );
}

#[test]
fn supersede_and_epoch_regression_fail_closed() {
    let notification = NotificationId::new();
    let replacement = NotificationId::new();
    let supersede = NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        1,
        4,
        NotificationLifecycleKind::Supersede,
        Some(replacement),
        "replacement",
    )
    .unwrap();
    let mut projector = NotificationProjector::new();
    projector
        .apply_committed(&[event()], 1, &[supersede])
        .unwrap();
    assert_eq!(
        projector.visibility(notification),
        NotificationVisibility::Superseded
    );
    let lower_epoch = NotificationLifecycleFact::new(
        NotificationId::new(),
        EventId::new(),
        2,
        3,
        NotificationLifecycleKind::Withdraw,
        None,
        "regression",
    )
    .unwrap();
    assert_eq!(
        projector.apply_committed(&[event()], 2, &[lower_epoch]),
        Err(NotificationProjectorError::EpochRegressed)
    );
}
