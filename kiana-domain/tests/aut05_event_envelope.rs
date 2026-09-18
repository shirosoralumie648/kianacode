use kiana_domain::{json_digest, AutomationEventEnvelope, RequestId};
use serde_json::json;

fn envelope() -> AutomationEventEnvelope {
    AutomationEventEnvelope::new(
        "owner:/repo",
        3,
        RequestId::new(),
        "command-3",
        json_digest(&json!({"command":"advance","version":3})),
        json_digest(&json!({"authority_epoch":4,"proof":[]})),
    )
    .unwrap()
}

#[test]
fn automation_event_envelope_round_trips_and_binds_cursor_command_and_payload() {
    let envelope = envelope();
    envelope.validate().unwrap();
    let encoded = serde_json::to_value(&envelope).unwrap();
    assert_eq!(
        serde_json::from_value::<AutomationEventEnvelope>(encoded).unwrap(),
        envelope
    );
    envelope
        .validate_against(
            "owner:/repo",
            3,
            envelope.request_id,
            "command-3",
            &envelope.command_digest,
            &envelope.payload_digest,
        )
        .unwrap();
}

#[test]
fn envelope_rejects_cursor_or_payload_drift_and_unknown_fields() {
    let envelope = envelope();
    assert_eq!(
        envelope
            .validate_against(
                "owner:/repo",
                4,
                envelope.request_id,
                "command-3",
                &envelope.command_digest,
                &envelope.payload_digest,
            )
            .unwrap_err(),
        "automation_event_envelope_binding_mismatch"
    );
    let mut unknown = serde_json::to_value(envelope).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationEventEnvelope>(unknown).is_err());
}
