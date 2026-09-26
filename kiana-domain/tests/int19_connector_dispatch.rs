use kiana_domain::{json_digest, ConnectorDispatchLifecycle, ConnectorDispatchStage, InvocationId};
use serde_json::json;

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn prepared() -> ConnectorDispatchLifecycle {
    ConnectorDispatchLifecycle::prepared(
        InvocationId::new(),
        1,
        digest("command"),
        digest("binding"),
        digest("payload"),
        digest("idempotency"),
    )
    .expect("prepared lifecycle")
}

#[test]
fn lifecycle_requires_prepared_dispatch_observe_commit_order() {
    let prepared = prepared();
    let dispatching = prepared
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching");
    let observed = dispatching
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed");
    let committed = observed
        .advance(
            ConnectorDispatchStage::ResultCommitted,
            observed.receipt_digest.clone(),
            observed.observation_digest.clone(),
            Some(digest("result")),
            None,
        )
        .expect("committed");

    assert_eq!(committed.stage, ConnectorDispatchStage::ResultCommitted);
    assert!(committed.stage.is_terminal());
    assert_eq!(committed.validate(), Ok(()));
    assert_eq!(
        committed
            .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
            .unwrap_err(),
        "connector_dispatch_terminal_resurrection"
    );
}

#[test]
fn unknown_is_terminal_and_may_keep_only_observation_evidence() {
    let dispatching = prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching");
    let unknown = dispatching
        .advance(
            ConnectorDispatchStage::Unknown,
            None,
            None,
            None,
            Some("adapter_timeout".to_owned()),
        )
        .expect("unknown");
    assert_eq!(unknown.stage, ConnectorDispatchStage::Unknown);
    assert_eq!(unknown.validate(), Ok(()));
    assert_eq!(
        unknown
            .advance(
                ConnectorDispatchStage::ResultCommitted,
                Some(digest("receipt")),
                Some(digest("observation")),
                Some(digest("result")),
                None,
            )
            .unwrap_err(),
        "connector_dispatch_terminal_resurrection"
    );

    let observed = prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching")
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed");
    let unknown_with_evidence = observed
        .advance(
            ConnectorDispatchStage::Unknown,
            observed.receipt_digest.clone(),
            observed.observation_digest.clone(),
            None,
            Some("provider_outcome_unknown".to_owned()),
        )
        .expect("unknown with evidence");
    assert_eq!(unknown_with_evidence.stage, ConnectorDispatchStage::Unknown);
    assert_eq!(unknown_with_evidence.validate(), Ok(()));
}

#[test]
fn malformed_phase_artifacts_and_unknown_fields_fail_closed() {
    let prepared = prepared();
    let dispatching = prepared
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching");
    assert_eq!(
        dispatching
            .advance(
                ConnectorDispatchStage::Observed,
                Some(digest("receipt")),
                None,
                None,
                None,
            )
            .unwrap_err(),
        "connector_dispatch_observation_incomplete"
    );

    let observed = dispatching
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed");
    assert_eq!(
        observed
            .advance(
                ConnectorDispatchStage::ResultCommitted,
                Some(digest("different_receipt")),
                observed.observation_digest.clone(),
                Some(digest("result")),
                None,
            )
            .unwrap_err(),
        "connector_dispatch_observation_changed"
    );

    let mut value = serde_json::to_value(prepared).expect("encode");
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorDispatchLifecycle>(value).is_err());
}
