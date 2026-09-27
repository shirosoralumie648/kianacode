#[test]
fn billing_invoice_keeps_authentication_identity_and_discrepancy_visible() {
    let domain = include_str!("../../kiana-domain/src/billing_invoice.rs");
    let core = include_str!("../src/billing_invoice.rs");
    for marker in [
        "ProviderInvoiceImport",
        "authenticated",
        "authentication_ref",
        "provider_invoice_duplicate",
        "InvoiceComparison",
        "period_mismatch",
        "usage_unknown",
        "correction_ref",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "BQ-22 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "overwrite_ledger",
        "auto_success",
        "invoice_paid",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "BQ-22 boundary executes effects: {forbidden}"
        );
    }
}
