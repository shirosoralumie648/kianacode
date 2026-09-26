#[test]
fn company_recovery_is_durable_fact_only_and_never_blindly_reexecutes() {
    let recovery = include_str!("../../kiana-domain/src/company_recovery.rs");
    let replay = include_str!("../../kiana-domain/src/company_replay.rs");
    let projection = include_str!("../../kiana-domain/src/projection_recovery.rs");
    let core = include_str!("../src/company_recovery.rs");
    for marker in [
        "COMPANY_RECOVERY_SCHEMA",
        "CompanyRecoveryFact",
        "CompanyRecoverySnapshot",
        "CompanyRecoveryPlan",
        "CompanyRecoveryLedger",
        "default_paused",
        "company_recovery_fact_gap_or_project_mismatch",
        "company_recovery_resume_blocked",
        "automatic_retry_allowed",
        "CompanyReplayReducer",
        "UnknownMutationReconciliation",
        "record_company_recovery_snapshot",
        "record_company_recovery_plan",
    ] {
        assert!(
            recovery.contains(marker)
                || replay.contains(marker)
                || projection.contains(marker)
                || core.contains(marker),
            "CO-42 marker missing: {marker}"
        );
    }
    for forbidden in [
        "automatic_retry_unknown",
        "retry_unknown_effect",
        "Command::new",
        "ModelClient::new",
    ] {
        assert!(
            !recovery.contains(forbidden),
            "CO-42 blind replay marker present: {forbidden}"
        );
    }
}
