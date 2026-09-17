use kiana_domain::{event_kind_spec, schema_contract, unknown_event_policy, RequestId};
use kiana_protocol::{
    QualityCommandKind, QualityCommandRequest, RequestBody, RequestEnvelope, RequestMetadata,
    QUALITY_COMMAND_KINDS, QUALITY_COMMAND_SCHEMA, QUALITY_EVENT_KINDS,
};
use serde_json::json;

#[test]
fn quality_protocol_has_no_unversioned_events() {
    assert_eq!(QUALITY_COMMAND_SCHEMA, "kiana.quality-command.v1");
    for name in QUALITY_COMMAND_KINDS {
        assert!(name.contains('.'));
    }
    for kind in QUALITY_EVENT_KINDS {
        assert!(
            event_kind_spec(kind).is_some(),
            "unregistered quality event: {kind}"
        );
        let payload = json!({"request_id": RequestId::new()});
        kiana_domain::validate_event_payload(kind, &payload).unwrap();
    }
    assert_eq!(
        unknown_event_policy("quality.future").unwrap_err(),
        "unknown_required_event_kind"
    );
    assert_eq!(
        schema_contract("kiana.quality-command.v1")
            .unwrap()
            .owner_crate,
        "kiana-protocol"
    );
}

#[test]
fn quality_command_is_a_typed_normal_route_and_rejects_bad_envelopes() {
    let request = QualityCommandRequest::new(
        QualityCommandKind::EvalRun,
        RequestId::new(),
        json!({"suite_id":"suite"}),
    );
    let envelope =
        RequestEnvelope::quality_command(RequestMetadata::local("s", "."), request).unwrap();
    match envelope.body {
        RequestBody::Command(command) => assert_eq!(command.name, "eval.run"),
        other => panic!("unexpected body: {other:?}"),
    }

    let mut bad = QualityCommandRequest::new(
        QualityCommandKind::Promote,
        RequestId::new(),
        json!({"candidate_id":"candidate"}),
    );
    bad.schema = "kiana.quality-command.v9".to_owned();
    assert_eq!(
        bad.validate().unwrap_err(),
        "quality_command_schema_invalid"
    );
    let non_object =
        QualityCommandRequest::new(QualityCommandKind::Rollback, RequestId::new(), json!([]));
    assert_eq!(
        non_object.validate().unwrap_err(),
        "quality_command_arguments_invalid"
    );
}
