use kiana_domain::{
    ConnectorIngressEvent, ConnectorIngressOccurrence, ConnectorIngressPolicy,
    ConnectorIngressProtocol,
};
use serde_json::json;

const SIG: &str = "ab";

fn policy() -> ConnectorIngressPolicy {
    ConnectorIngressPolicy::new(
        ConnectorIngressProtocol::Webhook,
        "source-1",
        "key-1",
        "tenant-1",
        "project-1",
        ["job.created".to_owned()],
        ["id".to_owned()],
        ["id".to_owned(), "value".to_owned()],
        100,
        4_096,
    )
    .unwrap()
}

fn event() -> ConnectorIngressEvent {
    ConnectorIngressEvent::new(
        ConnectorIngressProtocol::Webhook,
        "source-1",
        "key-1",
        "tenant-1",
        "project-1",
        "event-1",
        "job.created",
        1_000,
        "nonce-1",
        json!({"id": "job-1", "value": 1}),
        SIG.repeat(32),
    )
    .unwrap()
}

#[test]
fn ingress_policy_binds_tenant_payload_nonce_and_occurrence() {
    let policy = policy();
    let event = event();
    policy.matches(&event, 1_050).unwrap();
    let occurrence = ConnectorIngressOccurrence::from_verified(&event, &policy).unwrap();
    occurrence.validate().unwrap();
    assert!(occurrence.signature_digest.starts_with("sha256:"));
    assert_eq!(
        occurrence.occurrence_key,
        "connector-ingress:source-1:event-1"
    );
}

#[test]
fn wrong_tenant_clock_payload_or_unknown_fields_fail_closed() {
    let policy = policy();
    let mut event = event();
    event.tenant_id = "tenant-2".to_owned();
    assert_eq!(
        policy.matches(&event, 1_000).unwrap_err(),
        "connector_ingress_header_invalid"
    );

    let mut event = event();
    event.occurred_at_unix_ms = 10_000;
    event.ingress_digest = kiana_domain::json_digest(&json!({"tampered": true}));
    assert_eq!(
        policy.matches(&event, 1_000).unwrap_err(),
        "connector_ingress_header_invalid"
    );

    let mut event = event();
    event.payload = json!({"unexpected": true});
    event.payload_digest = kiana_domain::json_digest(&event.payload);
    event.ingress_digest = kiana_domain::json_digest(&json!({"tampered": true}));
    assert_eq!(
        policy.matches(&event, 1_000).unwrap_err(),
        "connector_ingress_header_invalid"
    );

    let mut value = serde_json::to_value(event()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorIngressEvent>(value).is_err());
}
