//! INT-27 Core read-only connector notification validation facade.

use kiana_domain::{validate_connector_notification_payload, NotificationEventClass};
use serde_json::Value;

pub fn validate_connector_notification(
    kind: &str,
    payload: &Value,
) -> Result<NotificationEventClass, String> {
    validate_connector_notification_payload(kind, payload)
}
