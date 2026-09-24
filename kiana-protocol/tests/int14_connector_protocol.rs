use kiana_protocol::{
    ConnectorCommand, ConnectorCommandRequest, RequestBody, RequestEnvelope, RequestMetadata,
    CONNECTOR_COMMAND_SCHEMA,
};
use serde_json::json;

#[test]
fn typed_connector_request_uses_the_shared_versioned_command_envelope() {
    let mut request = ConnectorCommandRequest::new(ConnectorCommand::Health);
    request.binding_id = Some("binding-demo".to_owned());
    let envelope =
        RequestEnvelope::connector_command(RequestMetadata::local("s", "/repo"), request).unwrap();
    assert!(
        matches!(envelope.body, RequestBody::Command(ref command) if command.name == "connector.health")
    );
    assert_eq!(
        envelope.body_arguments_schema(),
        Some(CONNECTOR_COMMAND_SCHEMA)
    );
    let encoded = serde_json::to_value(&envelope).unwrap();
    assert_eq!(encoded["body"]["request"]["arguments"]["command"], "health");
    assert_eq!(
        serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
        envelope
    );
}

#[test]
fn wire_authority_and_unknown_endpoint_fields_fail_closed() {
    let mut forged = json!({
        "schema": CONNECTOR_COMMAND_SCHEMA,
        "version": {"major": 1, "minor": 0},
        "command": "invoke",
        "binding_id": "binding-demo",
        "operation": "read",
        "payload": {},
        "idempotency_key": "key",
        "actor_id": "forged"
    });
    assert!(serde_json::from_value::<ConnectorCommandRequest>(forged.clone()).is_ok());
    forged["endpoint"] = json!("https://attacker.invalid");
    let decoded: ConnectorCommandRequest = serde_json::from_value(forged).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "connector_server_owned_override"
    );
}

trait EnvelopeArgumentsSchema {
    fn body_arguments_schema(&self) -> Option<&str>;
}

impl EnvelopeArgumentsSchema for RequestEnvelope {
    fn body_arguments_schema(&self) -> Option<&str> {
        match &self.body {
            RequestBody::Command(command) => command.arguments["schema"].as_str(),
            _ => None,
        }
    }
}
