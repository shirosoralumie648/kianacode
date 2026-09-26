#[test]
fn closing_receipt_keeps_all_close_kinds_and_effect_evidence_boundaries() {
    let receipt = include_str!("../../kiana-domain/src/closing_receipt.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let core = include_str!("../src/closing_receipt.rs");
    for marker in [
        "COMPANY_CLOSING_RECEIPT_SCHEMA",
        "CompanyClosingReceiptContract",
        "CompanyClosingReceiptLedger",
        "CompanyCloseKind",
        "Success",
        "Failure",
        "Cancelled",
        "Waived",
        "packet_attempt_refs",
        "all_runs_stopped",
        "unresolved_incidents",
        "closing_success_chain_incomplete",
        "closing_cancel_stop_required",
        "closing_waiver_requirements_missing",
        "closing_receipt_role_independence_invalid",
        "CloseProject",
        "BusinessCloseKind",
        "CompanyClosingReceipt",
        "record_closing_receipt",
        "EventLog",
    ] {
        assert!(
            receipt.contains(marker)
                || company.contains(marker)
                || closeout.contains(marker)
                || core.contains(marker),
            "CO-35 marker missing: {marker}"
        );
    }
    for forbidden in [
        "runtime_completed_is_success",
        "model_claimed_close",
        "Command::new",
    ] {
        assert!(
            !receipt.contains(forbidden),
            "CO-35 bypass marker present: {forbidden}"
        );
    }
}
