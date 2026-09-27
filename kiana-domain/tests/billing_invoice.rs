use kiana_domain::{
    InvoiceComparison, InvoiceComparisonState, Money, ProviderInvoiceImport, ProviderReceiptRef,
    INVOICE_COMPARISON_SCHEMA, PROVIDER_INVOICE_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn import_digest(import: &ProviderInvoiceImport) -> String {
    kiana_domain::json_digest(&json!({
        "schema": import.schema,
        "provider_account_ref": import.provider_account_ref,
        "invoice_id": import.invoice_id,
        "period_start_unix_ms": import.period_start_unix_ms,
        "period_end_unix_ms": import.period_end_unix_ms,
        "model": import.model,
        "usage_digest": import.usage_digest,
        "amount": import.amount,
        "receipt_ref": import.receipt_ref,
        "authentication_ref": import.authentication_ref,
        "authenticated": import.authenticated,
        "source_cursor": import.source_cursor,
        "import_key": import.import_key,
    }))
}

fn invoice(usage_digest: Option<&str>) -> ProviderInvoiceImport {
    let mut invoice = ProviderInvoiceImport {
        schema: PROVIDER_INVOICE_SCHEMA.to_owned(),
        provider_account_ref: "account:provider-1".to_owned(),
        invoice_id: "invoice:2026-09".to_owned(),
        period_start_unix_ms: 1_000,
        period_end_unix_ms: 2_000,
        model: Some("model-a".to_owned()),
        usage_digest: usage_digest.map(str::to_owned),
        amount: Money::new("USD", 100).unwrap(),
        receipt_ref: ProviderReceiptRef::new("receipt:provider-1").unwrap(),
        authentication_ref: "verified:provider-1".to_owned(),
        authenticated: true,
        source_cursor: 12,
        import_key: "import:provider-1:2026-09".to_owned(),
        import_digest: String::new(),
    };
    invoice.import_digest = import_digest(&invoice);
    invoice
}

fn comparison(state: InvoiceComparisonState) -> InvoiceComparison {
    let (invoice_end, invoice_usage, invoice_amount, codes, correction_ref) = match state {
        InvoiceComparisonState::Matched => (2_000, Some(D1.to_owned()), 100, vec![], None),
        InvoiceComparisonState::CorrectionRequired => (
            2_001,
            Some(D1.to_owned()),
            120,
            vec!["period_mismatch".to_owned(), "amount_mismatch".to_owned()],
            Some("correction:invoice-1".to_owned()),
        ),
        InvoiceComparisonState::ReviewRequired => {
            (2_000, None, 100, vec!["usage_unknown".to_owned()], None)
        }
    };
    let mut comparison = InvoiceComparison {
        schema: INVOICE_COMPARISON_SCHEMA.to_owned(),
        invoice_digest: D1.to_owned(),
        ledger_entry_digest: D2.to_owned(),
        expected_period_start_unix_ms: 1_000,
        expected_period_end_unix_ms: 2_000,
        invoice_period_start_unix_ms: 1_000,
        invoice_period_end_unix_ms: invoice_end,
        expected_model: "model-a".to_owned(),
        invoice_model: Some("model-a".to_owned()),
        expected_usage_digest: Some(D1.to_owned()),
        invoice_usage_digest: invoice_usage,
        expected_amount: Some(Money::new("USD", 100).unwrap()),
        invoice_amount: Money::new("USD", invoice_amount).unwrap(),
        state,
        discrepancy_codes: codes,
        correction_ref,
        comparison_digest: String::new(),
    };
    comparison.comparison_digest = kiana_domain::json_digest(&json!({
        "schema": comparison.schema,
        "invoice_digest": comparison.invoice_digest,
        "ledger_entry_digest": comparison.ledger_entry_digest,
        "expected_period_start_unix_ms": comparison.expected_period_start_unix_ms,
        "expected_period_end_unix_ms": comparison.expected_period_end_unix_ms,
        "invoice_period_start_unix_ms": comparison.invoice_period_start_unix_ms,
        "invoice_period_end_unix_ms": comparison.invoice_period_end_unix_ms,
        "expected_model": comparison.expected_model,
        "invoice_model": comparison.invoice_model,
        "expected_usage_digest": comparison.expected_usage_digest,
        "invoice_usage_digest": comparison.invoice_usage_digest,
        "expected_amount": comparison.expected_amount,
        "invoice_amount": comparison.invoice_amount,
        "state": comparison.state,
        "discrepancy_codes": comparison.discrepancy_codes,
        "correction_ref": comparison.correction_ref,
    }));
    comparison
}

#[test]
fn authenticated_invoice_import_is_deterministic_and_duplicates_fail_closed() {
    let imported = invoice(Some(D1));
    assert!(imported.validate().is_ok());
    assert!(kiana_domain::reject_duplicate_provider_invoices(&[imported.clone()]).is_ok());
    assert_eq!(
        kiana_domain::reject_duplicate_provider_invoices(&[imported.clone(), imported]),
        Err("provider_invoice_duplicate")
    );
}

#[test]
fn unauthenticated_or_malformed_receipts_are_rejected() {
    let mut invalid = invoice(Some(D1));
    invalid.authenticated = false;
    invalid.import_digest = import_digest(&invalid);
    assert_eq!(invalid.validate(), Err("provider_invoice_import_invalid"));

    let mut invalid = invoice(Some(D1));
    invalid.period_end_unix_ms = invalid.period_start_unix_ms;
    invalid.import_digest = import_digest(&invalid);
    assert_eq!(invalid.validate(), Err("provider_invoice_import_invalid"));

    let mut value = serde_json::to_value(invoice(Some(D1))).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ProviderInvoiceImport>(value).is_err());
}

#[test]
fn comparison_tracks_match_correction_and_unknown_review() {
    assert!(comparison(InvoiceComparisonState::Matched)
        .validate()
        .is_ok());
    assert!(comparison(InvoiceComparisonState::CorrectionRequired)
        .validate()
        .is_ok());
    assert!(comparison(InvoiceComparisonState::ReviewRequired)
        .validate()
        .is_ok());

    let mut forged = comparison(InvoiceComparisonState::CorrectionRequired);
    forged.correction_ref = None;
    forged.comparison_digest = kiana_domain::json_digest(&json!({
        "schema": forged.schema,
        "invoice_digest": forged.invoice_digest,
        "ledger_entry_digest": forged.ledger_entry_digest,
        "expected_period_start_unix_ms": forged.expected_period_start_unix_ms,
        "expected_period_end_unix_ms": forged.expected_period_end_unix_ms,
        "invoice_period_start_unix_ms": forged.invoice_period_start_unix_ms,
        "invoice_period_end_unix_ms": forged.invoice_period_end_unix_ms,
        "expected_model": forged.expected_model,
        "invoice_model": forged.invoice_model,
        "expected_usage_digest": forged.expected_usage_digest,
        "invoice_usage_digest": forged.invoice_usage_digest,
        "expected_amount": forged.expected_amount,
        "invoice_amount": forged.invoice_amount,
        "state": forged.state,
        "discrepancy_codes": forged.discrepancy_codes,
        "correction_ref": forged.correction_ref,
    }));
    assert_eq!(
        forged.validate(),
        Err("invoice_comparison_correction_required")
    );
}
