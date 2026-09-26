use kiana_domain::*;

fn fact(sequence: u64, cursor: u64, outcome: CompanyFactOutcome) -> CompanyRecoveryFact {
    let mut value = CompanyRecoveryFact {
        schema: COMPANY_FACT_SCHEMA.to_owned(),
        schema_version: COMPANY_RECOVERY_SCHEMA_VERSION,
        project_id: "project-1".to_owned(),
        sequence,
        source_cursor: cursor,
        kind: if sequence == 1 {
            CompanyFactKind::Intent
        } else {
            CompanyFactKind::RuntimeObservation
        },
        outcome,
        payload_digest: format!("sha256:{}", "p".repeat(64)),
        evidence_digest: (outcome == CompanyFactOutcome::Known)
            .then(|| format!("sha256:{}", "e".repeat(64))),
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn company_restart_with_missing_result_or_corrupt_evidence_never_reexecutes_blindly() {
    let facts = [
        fact(1, 10, CompanyFactOutcome::Known),
        fact(2, 11, CompanyFactOutcome::ResultUnknown),
    ];
    let snapshot = CompanyRecoverySnapshot::hydrate("project-1", 4, 7, 2, &facts).unwrap();
    assert_eq!(snapshot.state, CompanyRecoveryState::NeedsReconciliation);
    assert!(snapshot.default_paused);
    assert_eq!(
        CompanyRecoveryPlan::new(
            "resume-1",
            &snapshot,
            CompanyRecoveryAction::Resume,
            Some("approval:1".to_owned())
        )
        .unwrap_err(),
        "company_recovery_resume_blocked"
    );
    assert!(
        !CompanyRecoveryPlan::new("read-1", &snapshot, CompanyRecoveryAction::ReadOnly, None)
            .unwrap()
            .automatic_retry_allowed
    );
}

#[test]
fn new_process_rebuilds_and_resumes_company_chain_from_durable_facts() {
    let facts = [fact(1, 10, CompanyFactOutcome::Known)];
    let first = CompanyRecoverySnapshot::hydrate("project-1", 4, 7, 1, &facts).unwrap();
    let second = CompanyRecoverySnapshot::hydrate("project-1", 4, 7, 1, &facts).unwrap();
    assert_eq!(first, second);
    let plan = CompanyRecoveryPlan::new(
        "resume-1",
        &first,
        CompanyRecoveryAction::Resume,
        Some("approval:resume-1".to_owned()),
    )
    .expect("approved resume plan");
    let mut ledger = CompanyRecoveryLedger::default();
    ledger.record_snapshot(first.clone()).expect("snapshot");
    ledger.record_snapshot(second).expect("idempotent snapshot");
    ledger.record_plan(plan.clone()).expect("plan");
    ledger.record_plan(plan).expect("idempotent plan");
}

#[test]
fn recovery_rejects_cursor_gap_unknown_schema_and_stale_resume_material() {
    let gap = [
        fact(1, 10, CompanyFactOutcome::Known),
        fact(3, 12, CompanyFactOutcome::Known),
    ];
    assert_eq!(
        CompanyRecoverySnapshot::hydrate("project-1", 1, 1, 1, &gap).unwrap_err(),
        "company_recovery_fact_gap_or_project_mismatch"
    );
    let mut unknown = fact(1, 10, CompanyFactOutcome::Known);
    unknown.schema_version = 2;
    unknown.digest = unknown.canonical_digest();
    assert_eq!(
        CompanyRecoverySnapshot::hydrate("project-1", 1, 1, 1, &[unknown]).unwrap_err(),
        "company_recovery_fact_header_invalid"
    );
    let snapshot = CompanyRecoverySnapshot::hydrate(
        "project-1",
        1,
        1,
        1,
        &[fact(1, 10, CompanyFactOutcome::Known)],
    )
    .unwrap();
    let mut stale = CompanyRecoveryPlan::new(
        "resume-1",
        &snapshot,
        CompanyRecoveryAction::Resume,
        Some("approval:1".to_owned()),
    )
    .unwrap();
    stale.authority_epoch = 2;
    stale.digest = stale.canonical_digest();
    assert_eq!(
        stale.validate_against(&snapshot).unwrap_err(),
        "company_recovery_plan_binding_invalid"
    );
}
