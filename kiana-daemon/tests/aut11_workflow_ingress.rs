use kiana_daemon::WorkflowEventVerifier;
use kiana_domain::{WorkflowEventIngress, WorkflowEventSourcePolicy};
use ring::hmac;
use serde_json::json;

const SECRET: &[u8] = b"workflow-event-test-secret-32-bytes";

fn policy() -> WorkflowEventSourcePolicy {
    WorkflowEventSourcePolicy::new(
        "orders",
        "orders-key-1",
        "project-1",
        ["order.created".to_owned()],
        ["order_id".to_owned()],
        ["order_id".to_owned(), "amount".to_owned()],
        10_000,
    )
    .expect("valid policy")
}

fn unsigned_event(payload: serde_json::Value) -> WorkflowEventIngress {
    WorkflowEventIngress::new(
        "orders",
        "orders-key-1",
        "project-1",
        "trigger-orders",
        "evt-1",
        "order.created",
        1_000,
        payload,
        "0000000000000000000000000000000000000000000000000000000000000000",
    )
    .expect("valid event")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn signed_event(payload: serde_json::Value) -> WorkflowEventIngress {
    let draft = unsigned_event(payload);
    let key = hmac::Key::new(hmac::HMAC_SHA256, SECRET);
    let signature = hex(hmac::sign(&key, draft.signing_digest().as_bytes()).as_ref());
    WorkflowEventIngress::new(
        draft.source_id,
        draft.key_id,
        draft.project_id,
        draft.trigger_id,
        draft.event_id,
        draft.event_kind,
        draft.occurred_at_unix_ms,
        draft.payload,
        signature,
    )
    .expect("signed event")
}

#[test]
fn verifier_accepts_one_event_and_deduplicates_same_digest() {
    let verifier = WorkflowEventVerifier::new([(policy(), SECRET.to_vec())]).expect("verifier");
    let event = signed_event(json!({"order_id":"o-1","amount":3}));
    let first = verifier
        .verify(event.clone(), 1_000)
        .expect("verified")
        .expect("new occurrence");
    assert_eq!(first.occurrence_key, "event:orders:evt-1");
    assert!(verifier.verify(event, 1_000).expect("replay").is_none());
}

#[test]
fn verifier_rejects_bad_signature_source_project_and_payload_filter() {
    let verifier = WorkflowEventVerifier::new([(policy(), SECRET.to_vec())]).expect("verifier");

    let mut bad_signature = signed_event(json!({"order_id":"o-1"}));
    bad_signature.signature =
        "0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    bad_signature.ingress_digest = bad_signature.digest();
    assert!(verifier.verify(bad_signature, 1_000).is_err());

    let mut other_source = signed_event(json!({"order_id":"o-2"}));
    other_source.source_id = "untrusted".to_owned();
    other_source.ingress_digest = other_source.digest();
    assert!(verifier.verify(other_source, 1_000).is_err());

    let mut other_project = signed_event(json!({"order_id":"o-3"}));
    other_project.project_id = "project-2".to_owned();
    other_project.ingress_digest = other_project.digest();
    assert!(verifier.verify(other_project, 1_000).is_err());

    let extra = signed_event(json!({"order_id":"o-4","not_allowed":true}));
    assert!(verifier.verify(extra, 1_000).is_err());
}
