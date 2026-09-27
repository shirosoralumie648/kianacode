use kiana_domain::{
    validate_swarm_execution_route, AttemptId, ChildCellId, EventId, RunId, SessionId,
    SwarmExecutionRouteRequest, SWARM_ROUTE_SCHEMA,
};
use serde_json::json;

fn request() -> SwarmExecutionRouteRequest {
    let mut value = SwarmExecutionRouteRequest {
        schema: SWARM_ROUTE_SCHEMA.to_owned(),
        parent_run_id: RunId::new(),
        child_run_id: RunId::new(),
        child_session_id: SessionId::new("child-session"),
        child_cell_id: ChildCellId::new(),
        attempt_id: AttemptId::new(),
        partition_key: "partition-a".to_owned(),
        correlation_id: "corr-a".to_owned(),
        causation_event_id: EventId::new(),
        daemon_host_route: "DaemonHost".to_owned(),
        control_plane_route: "ControlPlane".to_owned(),
        broker_route: "CapabilityBroker".to_owned(),
        harness_route: "KianaHarness".to_owned(),
        direct_runner_route: false,
        direct_provider_route: false,
        authority_epoch: 4,
        route_digest: String::new(),
    };
    value.route_digest = value.canonical_digest();
    value
}

#[test]
fn child_route_binds_single_spine_and_correlation() {
    let receipt = validate_swarm_execution_route(&request()).unwrap();
    assert_eq!(receipt.correlation_id, "corr-a");
    assert!(!receipt.effect_dispatched);
    assert!(receipt.validate().is_ok());
}

#[test]
fn direct_runner_provider_routes_and_wrong_spine_fail_closed() {
    let mut direct = request();
    direct.direct_runner_route = true;
    direct.route_digest = direct.canonical_digest();
    assert_eq!(
        validate_swarm_execution_route(&direct),
        Err("swarm_execution_route_invalid")
    );

    let mut wrong = request();
    wrong.broker_route = "ProviderGateway".to_owned();
    wrong.route_digest = wrong.canonical_digest();
    assert_eq!(
        validate_swarm_execution_route(&wrong),
        Err("swarm_execution_route_invalid")
    );
}

#[test]
fn route_unknown_fields_and_digest_drift_fail_closed() {
    let mut value = serde_json::to_value(request()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmExecutionRouteRequest>(value).is_err());

    let mut drift = request();
    drift.route_digest =
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned();
    assert_eq!(
        validate_swarm_execution_route(&drift),
        Err("swarm_execution_route_invalid")
    );
}
