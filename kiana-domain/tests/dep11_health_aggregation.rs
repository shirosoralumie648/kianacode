use kiana_domain::{
    schema_contract, ComponentHealthState, EventId, HealthAggregationInput,
    HealthAggregationReport, HealthProbeKind, OperationPhase, OperationState, SignalStatus,
    StartupCoordinatorStatus, HEALTH_AGGREGATION_SCHEMA,
};

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn input(probe: HealthProbeKind) -> HealthAggregationInput {
    let mut input = HealthAggregationInput {
        schema: HEALTH_AGGREGATION_SCHEMA.to_owned(),
        version: kiana_domain::HEALTH_AGGREGATION_VERSION,
        probe,
        component: "control_plane".to_owned(),
        source_cursor: 4,
        source_event_ids: vec![EventId::new()],
        observed_at_ms: 1_000,
        authority_epoch: 4,
        data_epoch: 7,
        generation: 2,
        startup_status: StartupCoordinatorStatus::Ready,
        startup_report_digest: D.to_owned(),
        eventlog_ready: true,
        projection_cursor: 4,
        unknown_count: 0,
        lease_active: true,
        lease_authority_epoch: 4,
        lease_data_epoch: 7,
        lease_fence_valid: true,
        provider_healthy: true,
        provider_evidence_verified: true,
        drain_active: false,
        maintenance_active: false,
        operation_state: OperationState::Executing,
        operation_phase: OperationPhase::Executing,
        operation_deadline_expired: false,
        component_evidence_digest: D.to_owned(),
        input_digest: String::new(),
    };
    input.input_digest = input.digest();
    input.validate().unwrap();
    input
}

#[test]
fn health_aggregation_schema_and_verified_readiness_are_bound() {
    let contract = schema_contract(HEALTH_AGGREGATION_SCHEMA).unwrap();
    assert_eq!(contract.owner_crate, "kiana-domain");
    assert!(!contract.allow_unknown_fields);
    let input = input(HealthProbeKind::Readiness);
    let report = HealthAggregationReport::evaluate(&input).unwrap();
    assert_eq!(report.status, SignalStatus::Ok);
    assert!(report.admission_allowed);
    assert_eq!(report.projection_lag, 0);
    report.validate_against(&input).unwrap();
}

#[test]
fn provider_self_report_lag_and_lease_conflict_cannot_make_ready() {
    let mut provider = input(HealthProbeKind::Readiness);
    provider.provider_evidence_verified = false;
    provider.input_digest = provider.digest();
    let provider_report = HealthAggregationReport::evaluate(&provider).unwrap();
    assert_eq!(provider_report.status, SignalStatus::Degraded);
    assert!(!provider_report.admission_allowed);
    assert_eq!(
        provider_report.snapshot.components["provider"].state,
        ComponentHealthState::Unknown
    );

    let mut lag = input(HealthProbeKind::Readiness);
    lag.projection_cursor = 3;
    lag.input_digest = lag.digest();
    let lag_report = HealthAggregationReport::evaluate(&lag).unwrap();
    assert_eq!(lag_report.status, SignalStatus::Degraded);
    assert_eq!(lag_report.projection_lag, 1);
    assert!(!lag_report.admission_allowed);

    let mut lease = input(HealthProbeKind::Readiness);
    lease.lease_authority_epoch = 3;
    lease.input_digest = lease.digest();
    let lease_report = HealthAggregationReport::evaluate(&lease).unwrap();
    assert_eq!(lease_report.status, SignalStatus::Unknown);
    assert!(lease_report.lease_conflict);
    assert!(!lease_report.admission_allowed);
}

#[test]
fn all_probe_modes_preserve_unknown_drain_and_maintenance_semantics() {
    let mut startup = input(HealthProbeKind::Startup);
    startup.startup_status = StartupCoordinatorStatus::Blocked;
    startup.input_digest = startup.digest();
    assert_eq!(
        HealthAggregationReport::evaluate(&startup).unwrap().status,
        SignalStatus::Degraded
    );

    let mut unknown = input(HealthProbeKind::Liveness);
    unknown.operation_state = OperationState::Unknown;
    unknown.operation_phase = OperationPhase::Terminal;
    unknown.input_digest = unknown.digest();
    assert_eq!(
        HealthAggregationReport::evaluate(&unknown).unwrap().status,
        SignalStatus::Unknown
    );

    let mut drain = input(HealthProbeKind::Drain);
    drain.drain_active = true;
    drain.input_digest = drain.digest();
    let drain_report = HealthAggregationReport::evaluate(&drain).unwrap();
    assert_eq!(drain_report.status, SignalStatus::Degraded);
    assert!(!drain_report.admission_allowed);

    let mut maintenance = input(HealthProbeKind::Maintenance);
    maintenance.maintenance_active = true;
    maintenance.input_digest = maintenance.digest();
    let maintenance_report = HealthAggregationReport::evaluate(&maintenance).unwrap();
    assert_eq!(maintenance_report.status, SignalStatus::Degraded);
    assert!(!maintenance_report.admission_allowed);
}

#[test]
fn health_aggregation_denies_empty_source_and_tampered_report() {
    let mut empty = input(HealthProbeKind::Readiness);
    empty.source_event_ids.clear();
    empty.input_digest = empty.digest();
    assert_eq!(
        empty.validate().unwrap_err(),
        "health_aggregation_input_header_invalid"
    );

    let input = input(HealthProbeKind::Readiness);
    let mut report = HealthAggregationReport::evaluate(&input).unwrap();
    report.admission_allowed = false;
    report.report_digest = report.digest();
    assert_eq!(
        report.validate_against(&input).unwrap_err(),
        "health_aggregation_report_binding_mismatch"
    );
    let encoded = serde_json::to_string(&input).unwrap();
    assert_eq!(
        serde_json::from_str::<HealthAggregationInput>(&encoded).unwrap(),
        input
    );
}
