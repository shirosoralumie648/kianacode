use kiana_daemon::ConnectorIngressVerifier;
use kiana_domain::{ConnectorIngressEvent, ConnectorIngressPolicy, ConnectorIngressProtocol};
use kiana_ports::PortError;
use ring::hmac;
use serde_json::json;

const SECRET: &[u8] = b"0123456789abcdef-secret";

fn policy() -> ConnectorIngressPolicy {
    ConnectorIngressPolicy::new(
        ConnectorIngressProtocol::Webhook,
        "source-1",
        "key-1",
        "tenant-1",
        "project-1",
        ["job.created".to_owned()],
        ["id".to_owned()],
        ["id".to_owned()],
        100,
        4_096,
    )
    .unwrap()
}

fn signed_event(event_id: &str, value: i64) -> ConnectorIngressEvent {
    let mut event = ConnectorIngressEvent::new(
        ConnectorIngressProtocol::Webhook,
        "source-1",
        "key-1",
        "tenant-1",
        "project-1",
        event_id,
        "job.created",
        1_000,
        format!("nonce-{event_id}"),
        json!({"id": event_id, "value": value}),
        "00".repeat(32),
    )
    .unwrap();
    let key = hmac::Key::new(hmac::HMAC_SHA256, SECRET);
    let tag = hmac::sign(&key, event.signing_digest().as_bytes());
    event.signature = tag
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    event.ingress_digest = kiana_domain::json_digest(&json!({
        "schema": event.schema,
        "protocol": event.protocol,
        "source_id": event.source_id,
        "key_id": event.key_id,
        "tenant_id": event.tenant_id,
        "project_id": event.project_id,
        "event_id": event.event_id,
        "event_kind": event.event_kind,
        "occurred_at_unix_ms": event.occurred_at_unix_ms,
        "nonce": event.nonce,
        "payload": event.payload,
        "payload_digest": event.payload_digest,
        "signature": event.signature,
        "signature_algorithm": event.signature_algorithm,
    }));
    event
}

#[test]
fn verifier_authenticates_and_deduplicates_webhook_occurrences() {
    let verifier = ConnectorIngressVerifier::new([(policy(), SECRET.to_vec())]).unwrap();
    let event = signed_event("event-1", 1);
    let occurrence = verifier.verify(event.clone(), 1_050).unwrap().unwrap();
    assert_eq!(occurrence.event_id, "event-1");
    assert!(verifier.verify(event, 1_050).unwrap().is_none());

    let conflict = verifier.verify(signed_event("event-1", 2), 1_050);
    assert_eq!(
        conflict.unwrap_err(),
        PortError::Conflict("connector_ingress_dedupe_payload_conflict".to_owned())
    );
}

#[test]
fn verifier_rejects_bad_signature_or_unallowlisted_source() {
    let verifier = ConnectorIngressVerifier::new([(policy(), SECRET.to_vec())]).unwrap();
    let mut bad = signed_event("event-2", 1);
    bad.signature = "00".repeat(32);
    bad.ingress_digest = kiana_domain::json_digest(&json!({
        "schema": bad.schema,
        "protocol": bad.protocol,
        "source_id": bad.source_id,
        "key_id": bad.key_id,
        "tenant_id": bad.tenant_id,
        "project_id": bad.project_id,
        "event_id": bad.event_id,
        "event_kind": bad.event_kind,
        "occurred_at_unix_ms": bad.occurred_at_unix_ms,
        "nonce": bad.nonce,
        "payload": bad.payload,
        "payload_digest": bad.payload_digest,
        "signature": bad.signature,
        "signature_algorithm": bad.signature_algorithm,
    }));
    let error = verifier.verify(bad, 1_050).unwrap_err();
    assert_eq!(
        error,
        PortError::Conflict("connector_ingress_signature_invalid".to_owned())
    );

    let mut unknown = signed_event("event-3", 1);
    unknown.source_id = "source-unknown".to_owned();
    let error = verifier.verify(unknown, 1_050).unwrap_err();
    assert_eq!(
        error,
        PortError::Failed("connector_ingress_header_invalid".to_owned())
    );
}
