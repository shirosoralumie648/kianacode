//! Server-owned event classification for notification materialization.
//!
//! A notification is derived only from a registered committed event. Model/UI text and unknown
//! event kinds cannot self-report approval, completion or other critical facts.
use crate::RuntimeEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const NOTIFICATION_EVENT_REGISTRY_SCHEMA: &str = "kiana.notification-event-registry.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEventClass {
    Approval,
    RunTerminal,
    RunUnknown,
    Handoff,
    Status,
    Evidence,
    Incident,
    Reminder,
}

impl NotificationEventClass {
    pub const fn critical(self) -> bool {
        matches!(
            self,
            Self::Approval | Self::RunTerminal | Self::RunUnknown | Self::Handoff | Self::Incident
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEventSource {
    ControlPlane,
    EventLog,
    Model,
    Ui,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct NotificationEventSpec {
    pub kind: &'static str,
    pub class: NotificationEventClass,
    pub owner: &'static str,
    pub critical: bool,
}

const fn spec(kind: &'static str, class: NotificationEventClass) -> NotificationEventSpec {
    NotificationEventSpec {
        kind,
        class,
        owner: "control_plane",
        critical: class.critical(),
    }
}

pub const NOTIFICATION_EVENT_SPECS: &[NotificationEventSpec] = &[
    spec("approval.requested", NotificationEventClass::Approval),
    spec("approval.activated", NotificationEventClass::Approval),
    spec("approval.approved", NotificationEventClass::Approval),
    spec("approval.denied", NotificationEventClass::Approval),
    spec("approval.expired", NotificationEventClass::Approval),
    spec("approval.cancelled", NotificationEventClass::Approval),
    spec("approval.consumed", NotificationEventClass::Approval),
    spec(
        "approval.continuation_unavailable",
        NotificationEventClass::Approval,
    ),
    spec("run.completed", NotificationEventClass::RunTerminal),
    spec("run.failed", NotificationEventClass::RunTerminal),
    spec("run.cancelled", NotificationEventClass::RunTerminal),
    spec("run.result_unknown", NotificationEventClass::RunUnknown),
    spec(
        "capability.result_unknown",
        NotificationEventClass::RunUnknown,
    ),
    spec("communication.handoff", NotificationEventClass::Handoff),
    spec(
        "communication.handoff_acknowledged",
        NotificationEventClass::Handoff,
    ),
    spec(
        "communication.handoff_rejected",
        NotificationEventClass::Handoff,
    ),
    spec(
        "communication.status_report",
        NotificationEventClass::Status,
    ),
    spec("communication.evidence", NotificationEventClass::Evidence),
    spec("communication.incident", NotificationEventClass::Incident),
    spec(
        "communication.incident_escalated",
        NotificationEventClass::Incident,
    ),
    spec("notification.reminder", NotificationEventClass::Reminder),
];

pub fn notification_event_spec(kind: &str) -> Option<&'static NotificationEventSpec> {
    NOTIFICATION_EVENT_SPECS
        .iter()
        .find(|spec| spec.kind == kind)
}

fn required_notification_family(kind: &str) -> bool {
    [
        "approval.",
        "run.",
        "capability.",
        "communication.",
        "notification.",
    ]
    .iter()
    .any(|prefix| kind.starts_with(prefix))
}

pub fn notification_event_source(value: &str) -> Result<NotificationEventSource, String> {
    match value {
        "control_plane" => Ok(NotificationEventSource::ControlPlane),
        "eventlog" => Ok(NotificationEventSource::EventLog),
        "model" => Ok(NotificationEventSource::Model),
        "ui" => Ok(NotificationEventSource::Ui),
        _ => Err("notification_event_source_unknown".to_owned()),
    }
}

/// Classify and validate an event before a notification projector may consume it.
pub fn validate_notification_event(
    kind: &str,
    source: NotificationEventSource,
    payload: &Value,
) -> Result<NotificationEventClass, String> {
    let spec = notification_event_spec(kind).ok_or_else(|| {
        if required_notification_family(kind) {
            "notification_event_kind_unregistered".to_owned()
        } else {
            "notification_event_not_materializable".to_owned()
        }
    })?;
    if let Some(declared) = payload.get("source").and_then(Value::as_str) {
        if notification_event_source(declared)? != source {
            return Err("notification_event_source_mismatch".to_owned());
        }
    }
    if matches!(
        source,
        NotificationEventSource::Model | NotificationEventSource::Ui
    ) {
        return Err("notification_event_source_untrusted".to_owned());
    }
    if spec.critical {
        let object = payload
            .as_object()
            .ok_or_else(|| "notification_event_owner_required".to_owned())?;
        let owner_present = ["owner_id", "actor_id", "sender_id", "request_id"]
            .iter()
            .any(|field| {
                object
                    .get(*field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
            });
        if !owner_present {
            return Err("notification_event_owner_required".to_owned());
        }
    }
    Ok(spec.class)
}

/// Validate a committed RuntimeEvent with the server-owned source label.
pub fn validate_notification_runtime_event(
    event: &RuntimeEvent,
    source: NotificationEventSource,
) -> Result<Option<NotificationEventClass>, String> {
    if notification_event_spec(&event.kind).is_none() {
        if required_notification_family(&event.kind) {
            return Err("notification_event_kind_unregistered".to_owned());
        }
        return Ok(None);
    }
    validate_notification_event(&event.kind, source, &event.data).map(Some)
}
