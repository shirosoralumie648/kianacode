use kiana_domain::{
    schema_contract, DeploymentAdmissionDecision, DeploymentAdmissionInput,
    DeploymentAdmissionStatus, MaintenanceWindow, SignalStatus, DEPLOYMENT_ADMISSION_INPUT_SCHEMA,
    DEPLOYMENT_ADMISSION_VERSION, MAINTENANCE_WINDOW_SCHEMA,
};

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn input() -> DeploymentAdmissionInput {
    let mut input = DeploymentAdmissionInput {
        schema: DEPLOYMENT_ADMISSION_INPUT_SCHEMA.to_owned(),
        version: DEPLOYMENT_ADMISSION_VERSION,
        health_digest: D.to_owned(),
        health_status: SignalStatus::Ok,
        startup_ready: true,
        maintenance_window: None,
        now_unix_ms: 2_000,
        intake_paused: false,
        drain_active: false,
        drain_deadline_unix_ms: None,
        migration_active: false,
        backup_active: false,
        lease_valid: true,
        authority_epoch: 4,
        data_epoch: 7,
        lease_authority_epoch: 4,
        lease_data_epoch: 7,
        operation_unknown: false,
        source_cursor: 9,
        input_digest: String::new(),
    };
    input.input_digest = input.digest();
    input.validate().unwrap();
    input
}

#[test]
fn admission_schemas_and_ready_path_are_bound() {
    for schema in [
        MAINTENANCE_WINDOW_SCHEMA,
        DEPLOYMENT_ADMISSION_INPUT_SCHEMA,
        "kiana.deployment-admission-decision.v1",
    ] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    let input = input();
    let decision = DeploymentAdmissionDecision::evaluate(&input).unwrap();
    assert_eq!(decision.status, DeploymentAdmissionStatus::Ready);
    assert!(decision.allow_new_work);
    assert!(decision.allow_existing_work);
    decision.validate_against(&input).unwrap();
}

#[test]
fn maintenance_pause_drain_and_expiry_preserve_existing_work_boundary() {
    let mut windowed = input();
    windowed.maintenance_window =
        Some(MaintenanceWindow::new("window-1", 1_000, 3_000, 1).unwrap());
    windowed.input_digest = windowed.digest();
    let decision = DeploymentAdmissionDecision::evaluate(&windowed).unwrap();
    assert_eq!(decision.status, DeploymentAdmissionStatus::Maintenance);
    assert!(!decision.allow_new_work);
    assert!(decision.allow_existing_work);

    let mut expired = windowed.clone();
    expired.now_unix_ms = 3_000;
    expired.input_digest = expired.digest();
    let decision = DeploymentAdmissionDecision::evaluate(&expired).unwrap();
    assert_eq!(decision.reason, "maintenance_window_expired");
    assert!(!decision.allow_new_work);

    let mut paused = input();
    paused.intake_paused = true;
    paused.input_digest = paused.digest();
    let decision = DeploymentAdmissionDecision::evaluate(&paused).unwrap();
    assert_eq!(decision.status, DeploymentAdmissionStatus::Paused);
    assert!(!decision.allow_new_work);
    assert!(decision.allow_existing_work);

    let mut draining = input();
    draining.drain_active = true;
    draining.drain_deadline_unix_ms = Some(3_000);
    draining.input_digest = draining.digest();
    let decision = DeploymentAdmissionDecision::evaluate(&draining).unwrap();
    assert_eq!(decision.status, DeploymentAdmissionStatus::Draining);
    assert!(!decision.allow_new_work);
    assert!(decision.allow_existing_work);

    let mut expired_drain = draining;
    expired_drain.now_unix_ms = 3_000;
    expired_drain.input_digest = expired_drain.digest();
    let decision = DeploymentAdmissionDecision::evaluate(&expired_drain).unwrap();
    assert_eq!(decision.status, DeploymentAdmissionStatus::Unknown);
    assert!(!decision.allow_existing_work);

    let mut missing_deadline = input();
    missing_deadline.drain_active = true;
    missing_deadline.input_digest = missing_deadline.digest();
    assert_eq!(
        missing_deadline.validate().unwrap_err(),
        "deployment_admission_input_header_invalid"
    );

    let mut window_and_expired_drain = input();
    window_and_expired_drain.maintenance_window =
        Some(MaintenanceWindow::new("window-2", 1_000, 5_000, 1).unwrap());
    window_and_expired_drain.drain_active = true;
    window_and_expired_drain.drain_deadline_unix_ms = Some(2_000);
    window_and_expired_drain.input_digest = window_and_expired_drain.digest();
    let decision = DeploymentAdmissionDecision::evaluate(&window_and_expired_drain).unwrap();
    assert_eq!(decision.status, DeploymentAdmissionStatus::Unknown);
    assert_eq!(decision.reason, "drain_deadline_expired");
}

#[test]
fn admission_denies_health_lease_unknown_and_migration_conflicts() {
    let mut unhealthy = input();
    unhealthy.health_status = SignalStatus::Degraded;
    unhealthy.input_digest = unhealthy.digest();
    assert_eq!(
        DeploymentAdmissionDecision::evaluate(&unhealthy)
            .unwrap()
            .status,
        DeploymentAdmissionStatus::Blocked
    );

    let mut lease = input();
    lease.lease_authority_epoch = 3;
    lease.input_digest = lease.digest();
    let decision = DeploymentAdmissionDecision::evaluate(&lease).unwrap();
    assert_eq!(decision.status, DeploymentAdmissionStatus::Unknown);

    let mut unknown = input();
    unknown.operation_unknown = true;
    unknown.input_digest = unknown.digest();
    assert_eq!(
        DeploymentAdmissionDecision::evaluate(&unknown)
            .unwrap()
            .status,
        DeploymentAdmissionStatus::Unknown
    );

    let mut migration = input();
    migration.migration_active = true;
    migration.input_digest = migration.digest();
    let decision = DeploymentAdmissionDecision::evaluate(&migration).unwrap();
    assert_eq!(decision.reason, "admission_maintenance_conflict");
    assert!(!decision.allow_new_work);
}

#[test]
fn admission_tamper_and_window_digest_are_fail_closed() {
    let mut window = MaintenanceWindow::new("window-1", 1_000, 3_000, 1).unwrap();
    window.ends_at_unix_ms = 9_000;
    assert_eq!(
        window.validate().unwrap_err(),
        "maintenance_window_digest_mismatch"
    );

    let input = input();
    let mut decision = DeploymentAdmissionDecision::evaluate(&input).unwrap();
    decision.allow_new_work = false;
    decision.decision_digest = decision.digest();
    assert_eq!(
        decision.validate_against(&input).unwrap_err(),
        "deployment_admission_decision_binding_mismatch"
    );
}
