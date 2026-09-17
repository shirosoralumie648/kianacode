use kiana_protocol::{
    InjectRequest, RequestBody, RequestEnvelope, RequestMetadata, SteerRequest, TurnId,
};

#[test]
fn additive_steer_and_inject_requests_round_trip_without_legacy_drift() {
    let metadata = RequestMetadata::local("h19-session", "/h19");
    let turn_id = TurnId::new();
    let steer = RequestEnvelope::steer_run(
        metadata.clone(),
        kiana_protocol::RunId::new(),
        turn_id,
        "steer",
    );
    let inject = RequestEnvelope::inject_run(
        metadata,
        kiana_protocol::RunId::new(),
        "next-turn",
        "web",
        "context",
        Some(turn_id),
    );

    let steer_wire = serde_json::to_value(&steer).unwrap();
    let inject_wire = serde_json::to_value(&inject).unwrap();
    let decoded_steer: RequestEnvelope = serde_json::from_value(steer_wire).unwrap();
    let decoded_inject: RequestEnvelope = serde_json::from_value(inject_wire).unwrap();
    assert!(matches!(
        decoded_steer.body,
        RequestBody::Steer(SteerRequest { .. })
    ));
    assert!(matches!(
        decoded_inject.body,
        RequestBody::Inject(InjectRequest { .. })
    ));
}

#[test]
fn steer_and_inject_reject_unknown_fields() {
    let metadata = RequestMetadata::local("h19-session", "/h19");
    let request = RequestEnvelope::steer_run(
        metadata,
        kiana_protocol::RunId::new(),
        TurnId::new(),
        "steer",
    );
    let mut wire = serde_json::to_value(request).unwrap();
    wire["body"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RequestEnvelope>(wire).is_err());
}
