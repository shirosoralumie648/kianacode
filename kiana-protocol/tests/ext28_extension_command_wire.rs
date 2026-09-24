use kiana_protocol::{
    ExtensionCommand, ExtensionCommandRequest, RequestBody, RequestEnvelope, RequestMetadata,
};

#[test]
fn typed_extension_command_round_trips_through_generic_envelope() {
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Disable);
    request.extension_id = Some("example.skill".to_owned());
    request.expected_registry_version = Some(7);
    request.actor_id = Some("operator".to_owned());
    request.reason = Some("pause pending review".to_owned());
    request.idempotency_key = Some("disable-example-7".to_owned());

    let envelope = RequestEnvelope::extension_command(
        RequestMetadata::local("session", "/workspace"),
        request.clone(),
    )
    .expect("valid extension command");
    let RequestBody::Command(command) = &envelope.body else {
        panic!("typed extension command must use shared command route");
    };
    assert_eq!(command.name, "extension.disable");
    assert_eq!(command.arguments["action"], "disable");
    let encoded = serde_json::to_vec(&envelope).unwrap();
    let decoded: RequestEnvelope = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, envelope);
}
