#[test]
fn company_reconciliation_is_separate_from_risk_triggers_and_keeps_unknown_immutable() {
    let contract = include_str!("../../kiana-domain/src/company_reconciliation.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company_reconciliation.rs");
    let cancellation = include_str!("../../kiana-domain/src/cancellation.rs");
    let effect = include_str!("../../kiana-domain/src/effect_observation.rs");
    for marker in [
        "CompanyRiskTrigger",
        "CompanyIncident",
        "CompanyReconciliationCase",
        "CompanyReconciliationLedger",
        "CompanyEffectObservation",
        "CompanyObservationKind",
        "MissingResult",
        "WorkerLost",
        "EvidenceCorrupt",
        "BudgetUnknown",
        "original_unknown_digest",
        "company_reconciliation_model_or_status_claim_forbidden",
        "company_reconciliation_stop_unconfirmed",
        "company_reconciliation_unknown_evidence_forbidden",
        "automatic_retry_allowed",
        "dependent_work_blocked",
        "record_company_reconciliation",
        "RunCancellationFact",
        "EffectObservationState",
    ] {
        assert!(
            contract.contains(marker)
                || company.contains(marker)
                || core.contains(marker)
                || cancellation.contains(marker)
                || effect.contains(marker),
            "CO-32 marker missing: {marker}"
        );
    }
    for forbidden in [
        "automatic_retry_unknown",
        "retry_unknown_effect",
        "runtime_outcome_changed",
        "ModelClient::new",
        "CapabilityBroker::new",
    ] {
        assert!(
            !contract.contains(forbidden),
            "CO-32 bypass marker present: {forbidden}"
        );
    }
}
