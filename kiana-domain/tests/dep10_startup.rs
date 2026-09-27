use kiana_domain::{
    schema_contract, StartupCoordinatorInput, StartupCoordinatorReport, StartupCoordinatorRequest,
    StartupCoordinatorStatus, StartupFactStatus, StartupResumeMode, StartupStage, StartupStageFact,
    DEPLOYMENT_STARTUP_FACT_SCHEMA, DEPLOYMENT_STARTUP_REPORT_SCHEMA,
    DEPLOYMENT_STARTUP_REQUEST_SCHEMA,
};

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn facts(status: StartupFactStatus) -> Vec<StartupStageFact> {
    StartupStage::ALL
        .into_iter()
        .map(|stage| StartupStageFact::new(stage, status, D, 4, 7, 2, 1, "ok", "none").unwrap())
        .collect()
}

fn input() -> StartupCoordinatorInput {
    StartupCoordinatorInput {
        release_manifest_digest: D.to_owned(),
        storage_root_digest: D.to_owned(),
        project_trust_digest: D.to_owned(),
        project_trusted: true,
        lease_digest: D.to_owned(),
        store_digest: D.to_owned(),
        migration_registry_digest: D.to_owned(),
        projector_digest: D.to_owned(),
        capacity_digest: D.to_owned(),
        config_revision: D.to_owned(),
        authority_epoch: 4,
        data_epoch: 7,
        generation: 2,
        pending_operation: false,
        journal_valid: true,
        journal_digest: D.to_owned(),
        resume_mode: StartupResumeMode::Fresh,
        recovery_evidence_digest: None,
        facts: facts(StartupFactStatus::Ready),
    }
}

#[test]
fn startup_order_and_report_are_schema_bound() {
    for schema in [
        DEPLOYMENT_STARTUP_FACT_SCHEMA,
        DEPLOYMENT_STARTUP_REQUEST_SCHEMA,
        DEPLOYMENT_STARTUP_REPORT_SCHEMA,
    ] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    let request = StartupCoordinatorRequest::new(input()).unwrap();
    let report = StartupCoordinatorReport::evaluate(&request).unwrap();
    assert_eq!(report.status, StartupCoordinatorStatus::Ready);
    assert_eq!(report.completed_stages, StartupStage::ALL.to_vec());
    assert_eq!(report.blocked_stage, None);
    report.validate_against(&request).unwrap();
    let encoded = serde_json::to_string(&report).unwrap();
    assert_eq!(
        serde_json::from_str::<StartupCoordinatorReport>(&encoded).unwrap(),
        report
    );
}

#[test]
fn startup_denies_missing_untrusted_invalid_journal_and_epoch_facts() {
    let mut missing = input();
    missing.facts.pop();
    let missing = StartupCoordinatorRequest::new(missing).unwrap();
    let report = StartupCoordinatorReport::evaluate(&missing).unwrap();
    assert_eq!(report.status, StartupCoordinatorStatus::Blocked);
    assert_eq!(report.blocked_stage, Some(StartupStage::Capacity));

    let mut untrusted = input();
    untrusted.project_trusted = false;
    let untrusted = StartupCoordinatorRequest::new(untrusted).unwrap();
    let report = StartupCoordinatorReport::evaluate(&untrusted).unwrap();
    assert_eq!(report.blocked_stage, Some(StartupStage::Trust));
    assert_eq!(report.reason, "startup_project_untrusted");

    let mut invalid_journal = input();
    invalid_journal.journal_valid = false;
    let invalid_journal = StartupCoordinatorRequest::new(invalid_journal).unwrap();
    let report = StartupCoordinatorReport::evaluate(&invalid_journal).unwrap();
    assert_eq!(report.blocked_stage, Some(StartupStage::Store));
    assert_eq!(report.reason, "startup_journal_invalid");

    let mut stale = input();
    stale.facts[6].authority_epoch = 3;
    stale.facts[6].fact_digest = stale.facts[6].digest();
    let stale = StartupCoordinatorRequest::new(stale).unwrap();
    let report = StartupCoordinatorReport::evaluate(&stale).unwrap();
    assert_eq!(report.blocked_stage, Some(StartupStage::Projector));
    assert_eq!(report.reason, "startup_epoch_or_generation_mismatch");
}

#[test]
fn startup_keeps_unknown_and_pending_recovery_explicit() {
    let mut unknown = input();
    unknown.facts[5] = StartupStageFact::new(
        StartupStage::Migration,
        StartupFactStatus::Unknown,
        D,
        4,
        7,
        2,
        1,
        "migration_result_unknown",
        "reconcile_migration",
    )
    .unwrap();
    let unknown = StartupCoordinatorRequest::new(unknown).unwrap();
    let report = StartupCoordinatorReport::evaluate(&unknown).unwrap();
    assert_eq!(report.status, StartupCoordinatorStatus::Unknown);
    assert_eq!(report.blocked_stage, Some(StartupStage::Migration));

    let mut pending = input();
    pending.pending_operation = true;
    let pending = StartupCoordinatorRequest::new(pending).unwrap();
    let report = StartupCoordinatorReport::evaluate(&pending).unwrap();
    assert_eq!(report.status, StartupCoordinatorStatus::Blocked);
    assert_eq!(report.reason, "startup_resume_requires_explicit_recovery");

    let mut recovered = input();
    recovered.pending_operation = true;
    recovered.resume_mode = StartupResumeMode::ExplicitRecovery;
    recovered.recovery_evidence_digest = Some(D.to_owned());
    let recovered = StartupCoordinatorRequest::new(recovered).unwrap();
    let report = StartupCoordinatorReport::evaluate(&recovered).unwrap();
    assert_eq!(report.status, StartupCoordinatorStatus::Ready);
}

#[test]
fn startup_rejects_tampered_request_and_nonready_fact_explanation() {
    let mut tampered = StartupCoordinatorRequest::new(input()).unwrap();
    tampered.config_revision =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "deployment_startup_request_digest_mismatch"
    );

    assert_eq!(
        StartupStageFact::new(
            StartupStage::Root,
            StartupFactStatus::Blocked,
            D,
            4,
            7,
            2,
            1,
            "ok",
            "none",
        )
        .unwrap_err(),
        "deployment_startup_nonready_fact_explanation_invalid"
    );

    let request = StartupCoordinatorRequest::new(input()).unwrap();
    let mut wire = serde_json::to_value(&request).unwrap();
    wire["facts"].as_array_mut().unwrap().reverse();
    let out_of_order: StartupCoordinatorRequest = serde_json::from_value(wire).unwrap();
    assert_eq!(
        out_of_order.validate().unwrap_err(),
        "deployment_startup_request_header_invalid"
    );
}
