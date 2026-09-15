use kiana_domain::{EntryPointKind, RunId};
use kiana_protocol::{
    ParityRequest, RequestBody, RequestEnvelope, RequestMetadata, PROTOCOL_SCHEMA,
};
use serde_json::json;

#[test]
fn parity_request_round_trips_through_the_versioned_envelope() {
    let request = RequestEnvelope::parity(
        RequestMetadata::local("session-1", "/repo"),
        ParityRequest {
            entrypoint: EntryPointKind::Desktop,
            run_id: Some(RunId::new()),
        },
    );
    let encoded = serde_json::to_vec(&request).unwrap();
    let decoded: RequestEnvelope = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, request);
    assert_eq!(decoded.schema, PROTOCOL_SCHEMA);
    assert!(matches!(decoded.body, RequestBody::Parity(_)));
}

#[test]
fn parity_wire_rejects_unknown_fields_and_missing_entrypoint() {
    let unknown = json!({
        "entrypoint": "web",
        "run_id": null,
        "unexpected": true
    });
    assert!(serde_json::from_value::<ParityRequest>(unknown).is_err());
    let missing = json!({"run_id": null});
    assert!(serde_json::from_value::<ParityRequest>(missing).is_err());
}
