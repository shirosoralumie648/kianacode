#[test]
fn company_command_receipt_and_dispatch_intent_use_existing_event_store_cas() {
    let domain = include_str!("../../kiana-domain/src/company_receipts.rs");
    let company = include_str!("../src/company.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    for marker in [
        "CompanyCommandReceipt",
        "DispatchIntent",
        "kiana.company-command-receipt.v1",
        "protected.command",
        "ResultUnknown",
    ] {
        assert!(
            domain.contains(marker),
            "receipt contract marker missing: {marker}"
        );
    }
    for marker in [
        "CompanyCommandReceipt::command_id",
        "CompanyCommandReceipt::new",
        "company_dispatch_kind",
        "proof.dispatch_intent",
        "replayed",
    ] {
        assert!(
            company.contains(marker),
            "Company receipt path marker missing: {marker}"
        );
    }
    for marker in ["commit_transition", "read_command", "CommandReceipt"] {
        assert!(
            ports.contains(marker),
            "event store receipt marker missing: {marker}"
        );
    }
}
