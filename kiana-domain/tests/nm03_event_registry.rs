use kiana_domain::*;
use serde_json::json;

#[test]
fn notification_event_registry_classifies_only_registered_committed_kinds() {
    let cases = [
        ("approval.requested", NotificationEventClass::Approval),
        ("run.completed", NotificationEventClass::RunTerminal),
        ("run.result_unknown", NotificationEventClass::RunUnknown),
        ("communication.handoff", NotificationEventClass::Handoff),
        (
            "communication.status_report",
            NotificationEventClass::Status,
        ),
        ("communication.evidence", NotificationEventClass::Evidence),
        ("communication.incident", NotificationEventClass::Incident),
        ("notification.reminder", NotificationEventClass::Reminder),
    ];
    for (kind, class) in cases {
        let spec = notification_event_spec(kind).unwrap();
        assert_eq!(spec.class, class);
        assert_eq!(
            validate_notification_event(
                kind,
                NotificationEventSource::ControlPlane,
                &json!({"actor_id":"server"}),
            )
            .unwrap(),
            class
        );
    }
    assert_eq!(
        validate_notification_event(
            "approval.future",
            NotificationEventSource::ControlPlane,
            &json!({"actor_id":"server"}),
        )
        .unwrap_err(),
        "notification_event_kind_unregistered"
    );
    assert_eq!(
        validate_notification_event(
            "future.opaque",
            NotificationEventSource::ControlPlane,
            &json!({"actor_id":"server"}),
        )
        .unwrap_err(),
        "notification_event_not_materializable"
    );
}

#[test]
fn model_ui_self_report_and_ownerless_critical_events_are_rejected() {
    let payload = json!({"actor_id":"server", "source":"model"});
    assert_eq!(
        validate_notification_event("run.completed", NotificationEventSource::Model, &payload,)
            .unwrap_err(),
        "notification_event_source_untrusted"
    );
    assert_eq!(
        validate_notification_event(
            "approval.approved",
            NotificationEventSource::Ui,
            &json!({"actor_id":"server"}),
        )
        .unwrap_err(),
        "notification_event_source_untrusted"
    );
    assert_eq!(
        validate_notification_event("run.failed", NotificationEventSource::EventLog, &json!({}),)
            .unwrap_err(),
        "notification_event_owner_required"
    );
    assert_eq!(
        validate_notification_event(
            "run.completed",
            NotificationEventSource::ControlPlane,
            &json!({"actor_id":"server", "source":"ui"}),
        )
        .unwrap_err(),
        "notification_event_source_mismatch"
    );
    assert_eq!(
        notification_event_source("unknown").unwrap_err(),
        "notification_event_source_unknown"
    );
}

#[test]
fn runtime_event_classification_requires_a_registered_server_source() {
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "run.completed",
        json!({"run_id":RunId::new(), "actor_id":"builder"}),
    )
    .unwrap();
    assert_eq!(
        validate_notification_runtime_event(&event, NotificationEventSource::EventLog).unwrap(),
        Some(NotificationEventClass::RunTerminal)
    );
    assert_eq!(
        validate_notification_runtime_event(&event, NotificationEventSource::Model).unwrap_err(),
        "notification_event_source_untrusted"
    );
    let unregistered = RuntimeEvent::new(
        RequestId::new(),
        1,
        "communication.future",
        json!({"message":"future"}),
    )
    .unwrap();
    assert_eq!(
        validate_notification_runtime_event(&unregistered, NotificationEventSource::EventLog)
            .unwrap_err(),
        "notification_event_kind_unregistered"
    );
}
