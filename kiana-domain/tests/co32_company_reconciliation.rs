use kiana_domain::*;

fn sha(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn trigger() -> CompanyRiskTrigger {
    let mut value = CompanyRiskTrigger {
        schema: COMPANY_RECONCILIATION_SCHEMA.to_owned(),
        trigger_id: "trigger-1".to_owned(),
        project_id: "project-1".to_owned(),
        packet_id: Some("packet-1".to_owned()),
        run_id: Some("run-1".to_owned()),
        delivery_id: None,
        kind: CompanyRiskTriggerKind::MissingResult,
        source_ref: "event:run-unknown-1".to_owned(),
        summary: "run result was not observed".to_owned(),
        observed_at: 10,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn incident() -> CompanyIncident {
    let mut value = CompanyIncident {
        schema: COMPANY_RECONCILIATION_SCHEMA.to_owned(),
        incident_id: "incident-1".to_owned(),
        trigger_id: "trigger-1".to_owned(),
        project_id: "project-1".to_owned(),
        packet_id: Some("packet-1".to_owned()),
        run_id: Some("run-1".to_owned()),
        delivery_id: None,
        owner_id: "operator-1".to_owned(),
        deadline_at: 100,
        original_unknown_digest: sha('u'),
        recovery_plan_ref: "recovery:run-1".to_owned(),
        state: CompanyIncidentState::ReconciliationPending,
        automatic_retry_allowed: false,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn case() -> CompanyReconciliationCase {
    CompanyReconciliationCase::from_incident(
        "case-1",
        &incident(),
        "run:run-1",
        "account:local",
        sha('s'),
        4,
        9,
    )
    .expect("case")
}

fn observation(outcome: CompanyObservationOutcome) -> CompanyEffectObservation {
    let mut value = CompanyEffectObservation {
        schema: COMPANY_RECONCILIATION_SCHEMA.to_owned(),
        observation_id: "observation-1".to_owned(),
        kind: CompanyObservationKind::Run,
        project_id: "project-1".to_owned(),
        packet_id: Some("packet-1".to_owned()),
        run_id: Some("run-1".to_owned()),
        delivery_id: None,
        target_ref: "run:run-1".to_owned(),
        account_ref: "account:local".to_owned(),
        scope_digest: sha('s'),
        baseline_version: 4,
        authority_epoch: 9,
        original_unknown_digest: sha('u'),
        source: CompanyObservationSource::IndependentQuery,
        outcome,
        evidence_refs: vec!["evidence:query-1".to_owned()],
        stop_confirmed: true,
        model_claimed: false,
        status_report: false,
        observed_at: 20,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn unknown_effect_cannot_be_retried_or_closed_by_status_report() {
    let mut ledger = CompanyReconciliationLedger::default();
    ledger.record_trigger(trigger()).expect("trigger");
    ledger.open_incident(incident()).expect("incident");
    let pending = case();
    ledger.record_case(pending.clone()).expect("pending");

    let mut claimed = observation(CompanyObservationOutcome::ConfirmedSucceeded);
    claimed.source = CompanyObservationSource::ModelReport;
    claimed.model_claimed = true;
    claimed.status_report = true;
    claimed.digest = claimed.canonical_digest();
    assert_eq!(
        pending.attach_observation(claimed).unwrap_err(),
        "company_reconciliation_model_or_status_claim_forbidden"
    );
    assert_eq!(
        pending.commit_reconciled().unwrap_err(),
        "company_reconciliation_evidence_required"
    );
    assert!(!pending.automatic_retry_allowed);
}

#[test]
fn wrong_object_account_scope_and_unconfirmed_stop_are_rejected() {
    let pending = case();
    let mut wrong = observation(CompanyObservationOutcome::ConfirmedSucceeded);
    wrong.target_ref = "run:other".to_owned();
    wrong.digest = wrong.canonical_digest();
    assert_eq!(
        pending.attach_observation(wrong).unwrap_err(),
        "company_reconciliation_evidence_binding_mismatch"
    );

    let mut account = observation(CompanyObservationOutcome::ConfirmedSucceeded);
    account.account_ref = "account:other".to_owned();
    account.digest = account.canonical_digest();
    assert_eq!(
        pending.attach_observation(account).unwrap_err(),
        "company_reconciliation_evidence_binding_mismatch"
    );

    let mut scope = observation(CompanyObservationOutcome::ConfirmedSucceeded);
    scope.scope_digest = sha('x');
    scope.digest = scope.canonical_digest();
    assert_eq!(
        pending.attach_observation(scope).unwrap_err(),
        "company_reconciliation_evidence_binding_mismatch"
    );

    let mut unstopped = observation(CompanyObservationOutcome::ConfirmedSucceeded);
    unstopped.stop_confirmed = false;
    unstopped.digest = unstopped.canonical_digest();
    assert_eq!(
        pending.attach_observation(unstopped).unwrap_err(),
        "company_reconciliation_stop_unconfirmed"
    );
}

#[test]
fn independent_observation_resolves_without_rewriting_original_unknown_and_preserves_history() {
    let mut ledger = CompanyReconciliationLedger::default();
    ledger.record_trigger(trigger()).expect("trigger");
    ledger.open_incident(incident()).expect("incident");
    let pending = case();
    ledger.record_case(pending.clone()).expect("pending");
    let evidence = pending
        .attach_observation(observation(CompanyObservationOutcome::ConfirmedSucceeded))
        .expect("evidence");
    ledger.record_case(evidence.clone()).expect("evidence fact");
    let resolved = evidence.commit_reconciled().expect("reconciled");
    ledger
        .record_case(resolved.clone())
        .expect("successor fact");
    let latest = ledger.latest_case("case-1").expect("latest");
    assert_eq!(latest.state, CompanyReconciliationState::Reconciled);
    assert!(latest.can_resume_dependents());
    assert_eq!(latest.original_unknown_digest, sha('u'));
    assert_eq!(
        latest.evidence.as_ref().unwrap().evidence_refs,
        ["evidence:query-1"]
    );
    assert_eq!(ledger.cases["case-1"].len(), 3);
}

#[test]
fn confirmed_failure_blocks_dependents_and_replay_is_idempotent() {
    let mut ledger = CompanyReconciliationLedger::default();
    ledger.record_trigger(trigger()).expect("trigger");
    ledger.open_incident(incident()).expect("incident");
    let pending = case();
    ledger.record_case(pending.clone()).expect("pending");
    let evidence = pending
        .attach_observation(observation(CompanyObservationOutcome::ConfirmedFailed))
        .expect("evidence");
    ledger.record_case(evidence.clone()).expect("evidence");
    let failed = evidence
        .commit_failed("independent query confirmed failure")
        .expect("failed");
    ledger.record_case(failed.clone()).expect("failed");
    ledger.record_case(failed).expect("idempotent replay");
    let latest = ledger.latest_case("case-1").unwrap();
    assert_eq!(latest.state, CompanyReconciliationState::Failed);
    assert!(latest.dependent_work_blocked);
    assert!(!latest.can_resume_dependents());
}
