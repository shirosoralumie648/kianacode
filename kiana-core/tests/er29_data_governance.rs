use kiana_core::{
    receipt_data_binding_from_events, receipt_redaction_is_not_authorization,
    seal_governance_events,
};
use kiana_domain::{DataPayloadState, DataPolicy, RequestId, RuntimeEvent};
use serde_json::json;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

#[test]
fn receipt_binding_keeps_payload_unknown_without_governance_authorization() {
    let event = RuntimeEvent::new(RequestId::new(), 1, "run.completed", json!({"safe": true}))
        .unwrap()
        .with_redaction_metadata(digest('a'), false, Some(1), vec!["artifact:one".to_owned()]);
    let binding = receipt_data_binding_from_events(
        &digest('b'),
        "/project",
        &DataPolicy::default(),
        &[event],
    )
    .unwrap();
    assert_eq!(binding.payload_state, DataPayloadState::Unknown);
    assert!(receipt_redaction_is_not_authorization(&binding, 1).is_err());
    assert!(binding
        .redact_payload_refs()
        .unwrap()
        .payload_refs
        .is_empty());
}

#[test]
fn governance_event_seal_has_no_payload_mutation_path() {
    let event =
        RuntimeEvent::new(RequestId::new(), 1, "run.completed", json!({"safe": true})).unwrap();
    let seal = seal_governance_events(&[event], 2, "sealed-audit").unwrap();
    assert!(seal.sealed);
    assert_eq!(seal.data_epoch, 2);
    assert!(seal.validate().is_ok());
}
