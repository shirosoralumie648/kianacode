use kiana_domain::{
    AutomationCommand, WorkflowEventIngress, WorkflowEventOccurrence, WorkflowEventSourcePolicy,
};
use serde_json::json;

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

fn event(payload: serde_json::Value) -> WorkflowEventIngress {
    WorkflowEventIngress::new(
        "orders",
        "orders-key-1",
        "project-1",
        "trigger-orders",
        "evt-1",
        "order.created",
        1_000,
        payload,
        "signature",
    )
    .expect("valid event")
}

#[test]
fn event_filter_and_occurrence_bind_only_allowlisted_payload_fields() {
    let ingress = event(json!({"order_id":"o-1","amount":3}));
    policy().matches(&ingress, 1_000).expect("filter matches");
    let occurrence =
        WorkflowEventOccurrence::from_verified(&ingress, &policy()).expect("occurrence");
    assert_eq!(occurrence.occurrence_key, "event:orders:evt-1");
    let fire = occurrence
        .to_fire_command("event:committed-1")
        .expect("fire material");
    assert!(matches!(
        fire,
        AutomationCommand::Fire {
            event_ref: Some(_),
            ..
        }
    ));
}

#[test]
fn wrong_kind_missing_field_extra_field_and_digest_tamper_fail_closed() {
    let mut wrong_kind = event(json!({"order_id":"o-1"}));
    wrong_kind.event_kind = "order.deleted".to_owned();
    wrong_kind.ingress_digest = wrong_kind.digest();
    assert_eq!(
        policy()
            .matches(&wrong_kind, 1_000)
            .expect_err("wrong kind"),
        "workflow_event_source_or_kind_denied"
    );

    let missing = event(json!({"amount":3}));
    assert_eq!(
        policy().matches(&missing, 1_000).expect_err("missing key"),
        "workflow_event_payload_filter_denied"
    );

    let extra = event(json!({"order_id":"o-1","secret":"do-not-forward"}));
    assert_eq!(
        policy().matches(&extra, 1_000).expect_err("extra key"),
        "workflow_event_payload_filter_denied"
    );

    let mut tampered = event(json!({"order_id":"o-1"}));
    tampered.payload = json!({"order_id":"other"});
    assert_eq!(
        tampered.validate().expect_err("payload tamper"),
        "workflow_event_payload_digest_mismatch"
    );
}
