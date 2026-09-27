//! BQ-22 read-only Core facade for provider invoice imports and comparison facts.

use kiana_domain::{
    reject_duplicate_provider_invoices as reject_domain_duplicates,
    validate_invoice_comparison as validate_domain_comparison,
    validate_provider_invoice as validate_domain_invoice, InvoiceComparison, ProviderInvoiceImport,
};

pub fn validate_provider_invoice(import: &ProviderInvoiceImport) -> Result<(), &'static str> {
    validate_domain_invoice(import)
}

pub fn reject_duplicate_provider_invoices(
    imports: &[ProviderInvoiceImport],
) -> Result<(), &'static str> {
    reject_domain_duplicates(imports)
}

pub fn validate_invoice_comparison(comparison: &InvoiceComparison) -> Result<(), &'static str> {
    validate_domain_comparison(comparison)
}
