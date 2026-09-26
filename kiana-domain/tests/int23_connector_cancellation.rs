use kiana_domain::{
    connector_not_executed_stop_report, json_digest, ConnectorCancellationSettlement,
    ConnectorCancellationState, ConnectorDispatchLifecycle, ConnectorDispatchStage,
    ConnectorLeaseSettlement, InvocationId, ProcessGroupState, StopMethod, StopReport,
};
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
    .expect("prepared")
}

#[test]
fn confirmed_stop_before_dispatch_releases_lease_as_not_executed() {
    let lifecycle = prepared();
    let report = connector_not_executed_stop_report(lifecycle.invocation_id).expect("stop");
    let settlement = ConnectorCancellationSettlement::from_stop_report(
        &lifecycle,
        digest("permit"),
        digest("lease"),
        &report,
        1_000,
        1_001,
    )
    .expect("settlement");
    assert_eq!(settlement.state, ConnectorCancellationState::NotExecuted);
    assert_eq!(
        settlement.lease_settlement,
        ConnectorLeaseSettlement::Released
    );
    assert_eq!(
        settlement.reject_late_result(&digest("late")).unwrap_err(),
        "connector_cancellation_late_result_fenced"
    );
}

#[test]
fn started_effect_keeps_lease_held_and_unconfirmed_stop_is_unknown() {
    let lifecycle = prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching");
    let confirmed = StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::Kill,
        true,
        true,
        true,
        ProcessGroupState::Empty,
        true,
        25,
    )
    .expect("confirmed stop");
    let stopped = ConnectorCancellationSettlement::from_stop_report(
        &lifecycle,
        digest("permit"),
        digest("lease"),
        &confirmed,
        1_000,
        1_025,
    )
    .expect("stopped");
    assert_eq!(stopped.state, ConnectorCancellationState::StopConfirmed);
    assert_eq!(
        stopped.lease_settlement,
        ConnectorLeaseSettlement::HeldForReconciliation
    );

    let unknown_report = StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::Term,
        true,
        false,
        false,
        ProcessGroupState::Unknown,
        false,
        25,
    )
    .expect("unknown stop");
    let unknown = ConnectorCancellationSettlement::from_stop_report(
        &lifecycle,
        digest("permit"),
        digest("lease"),
        &unknown_report,
        1_000,
        1_025,
    )
    .expect("unknown");
    assert_eq!(unknown.state, ConnectorCancellationState::Unknown);
    assert_eq!(
        unknown.lease_settlement,
        ConnectorLeaseSettlement::HeldForReconciliation
    );
}

#[test]
fn observed_or_malformed_late_material_cannot_be_cancelled_as_not_executed() {
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
    let report = connector_not_executed_stop_report(observed.invocation_id).expect("stop");
    assert_eq!(
        ConnectorCancellationSettlement::from_stop_report(
            &observed,
            digest("permit"),
            digest("lease"),
            &report,
            1,
            2,
        )
        .unwrap_err(),
        "connector_cancellation_effect_already_observed"
    );

    let prepared = prepared();
    let mut value = serde_json::to_value(
        ConnectorCancellationSettlement::from_stop_report(
            &prepared,
            digest("permit"),
            digest("lease"),
            &connector_not_executed_stop_report(prepared.invocation_id).unwrap(),
            1,
            2,
        )
        .unwrap(),
    )
    .expect("encode");
    value["late_provider_result"] = json!(true);
    assert!(serde_json::from_value::<ConnectorCancellationSettlement>(value).is_err());
}
