//! BQ-22 provider invoice import and reconciliation source contracts.
//!
//! An authenticated invoice is an external observation, not a replacement for the EventLog
//! ledger.  Imports are immutable and duplicate invoice identities are rejected before a caller
//! can route a correction through the existing BQ-14 approval command.  Missing or mismatched
//! fields remain visible as review/correction states; this module never silently overwrites a
//! measured ledger value.

use crate::{json_digest, Money, ProviderReceiptRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PROVIDER_INVOICE_SCHEMA: &str = "kiana.provider-invoice.v1";
pub const INVOICE_COMPARISON_SCHEMA: &str = "kiana.invoice-comparison.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInvoiceImport {
    pub schema: String,
    pub provider_account_ref: String,
    pub invoice_id: String,
    pub period_start_unix_ms: u64,
    pub period_end_unix_ms: u64,
    pub model: Option<String>,
    pub usage_digest: Option<String>,
    pub amount: Money,
    pub receipt_ref: ProviderReceiptRef,
    pub authentication_ref: String,
    pub authenticated: bool,
    pub source_cursor: u64,
    pub import_key: String,
    pub import_digest: String,
}

impl ProviderInvoiceImport {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PROVIDER_INVOICE_SCHEMA
            || !valid_text(&self.provider_account_ref)
            || !valid_text(&self.invoice_id)
            || self.period_start_unix_ms == 0
            || self.period_end_unix_ms <= self.period_start_unix_ms
            || self
                .model
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || self
                .usage_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self.amount.validate().is_err()
            || self.amount.micros < 0
            || ProviderReceiptRef::new(self.receipt_ref.as_str().to_owned()).is_err()
            || !valid_text(&self.authentication_ref)
            || !self.authenticated
            || self.source_cursor == 0
            || !valid_text(&self.import_key)
            || !valid_digest(&self.import_digest)
            || self.import_digest != self.digest()
        {
            return Err("provider_invoice_import_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "provider_account_ref": self.provider_account_ref,
            "invoice_id": self.invoice_id,
            "period_start_unix_ms": self.period_start_unix_ms,
            "period_end_unix_ms": self.period_end_unix_ms,
            "model": self.model,
            "usage_digest": self.usage_digest,
            "amount": self.amount,
            "receipt_ref": self.receipt_ref,
            "authentication_ref": self.authentication_ref,
            "authenticated": self.authenticated,
            "source_cursor": self.source_cursor,
            "import_key": self.import_key,
        }))
    }

    fn identity(&self) -> (&str, &str, u64, u64) {
        (
            &self.provider_account_ref,
            &self.invoice_id,
            self.period_start_unix_ms,
            self.period_end_unix_ms,
        )
    }
}

pub fn validate_provider_invoice(import: &ProviderInvoiceImport) -> Result<(), &'static str> {
    import.validate()
}

pub fn reject_duplicate_provider_invoices(
    imports: &[ProviderInvoiceImport],
) -> Result<(), &'static str> {
    let mut identities = BTreeSet::new();
    for import in imports {
        import.validate()?;
        if !identities.insert(import.identity()) {
            return Err("provider_invoice_duplicate");
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceComparisonState {
    Matched,
    CorrectionRequired,
    ReviewRequired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvoiceComparison {
    pub schema: String,
    pub invoice_digest: String,
    pub ledger_entry_digest: String,
    pub expected_period_start_unix_ms: u64,
    pub expected_period_end_unix_ms: u64,
    pub invoice_period_start_unix_ms: u64,
    pub invoice_period_end_unix_ms: u64,
    pub expected_model: String,
    pub invoice_model: Option<String>,
    pub expected_usage_digest: Option<String>,
    pub invoice_usage_digest: Option<String>,
    pub expected_amount: Option<Money>,
    pub invoice_amount: Money,
    pub state: InvoiceComparisonState,
    pub discrepancy_codes: Vec<String>,
    pub correction_ref: Option<String>,
    pub comparison_digest: String,
}

impl InvoiceComparison {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != INVOICE_COMPARISON_SCHEMA
            || !valid_digest(&self.invoice_digest)
            || !valid_digest(&self.ledger_entry_digest)
            || self.expected_period_start_unix_ms == 0
            || self.expected_period_end_unix_ms <= self.expected_period_start_unix_ms
            || self.invoice_period_start_unix_ms == 0
            || self.invoice_period_end_unix_ms <= self.invoice_period_start_unix_ms
            || !valid_text(&self.expected_model)
            || self
                .invoice_model
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || self
                .expected_usage_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self
                .invoice_usage_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self
                .expected_amount
                .as_ref()
                .is_some_and(|value| value.validate().is_err() || value.micros < 0)
            || self.invoice_amount.validate().is_err()
            || self.invoice_amount.micros < 0
            || self
                .expected_amount
                .as_ref()
                .is_some_and(|value| value.currency != self.invoice_amount.currency)
            || self.discrepancy_codes.len() > 8
            || self.discrepancy_codes.iter().any(|code| !valid_text(code))
            || has_duplicates(&self.discrepancy_codes)
            || self
                .correction_ref
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || !valid_digest(&self.comparison_digest)
            || self.comparison_digest != self.digest()
        {
            return Err("invoice_comparison_invalid");
        }

        let mismatches = self.mismatch_codes();
        match self.state {
            InvoiceComparisonState::Matched => {
                if !mismatches.is_empty()
                    || self.expected_usage_digest.is_none()
                    || self.invoice_usage_digest.is_none()
                    || !self.discrepancy_codes.is_empty()
                    || self.correction_ref.is_some()
                {
                    return Err("invoice_comparison_match_invalid");
                }
            }
            InvoiceComparisonState::CorrectionRequired => {
                if mismatches.is_empty()
                    || self.discrepancy_codes != mismatches
                    || self.correction_ref.is_none()
                {
                    return Err("invoice_comparison_correction_required");
                }
            }
            InvoiceComparisonState::ReviewRequired => {
                if mismatches.is_empty()
                    || self.discrepancy_codes != mismatches
                    || self.correction_ref.is_some()
                {
                    return Err("invoice_comparison_review_required");
                }
            }
        }
        Ok(())
    }

    fn mismatch_codes(&self) -> Vec<String> {
        let mut codes = Vec::new();
        if self.expected_period_start_unix_ms != self.invoice_period_start_unix_ms
            || self.expected_period_end_unix_ms != self.invoice_period_end_unix_ms
        {
            codes.push("period_mismatch".to_owned());
        }
        if self.invoice_model.as_deref() != Some(self.expected_model.as_str()) {
            codes.push("model_mismatch".to_owned());
        }
        match (
            self.expected_usage_digest.as_deref(),
            self.invoice_usage_digest.as_deref(),
        ) {
            (Some(expected), Some(invoice)) if expected == invoice => {}
            (Some(_), Some(_)) => codes.push("usage_mismatch".to_owned()),
            _ => codes.push("usage_unknown".to_owned()),
        }
        match self.expected_amount.as_ref() {
            Some(expected) if expected == &self.invoice_amount => {}
            Some(_) => codes.push("amount_mismatch".to_owned()),
            None => codes.push("amount_unknown".to_owned()),
        }
        codes
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "invoice_digest": self.invoice_digest,
            "ledger_entry_digest": self.ledger_entry_digest,
            "expected_period_start_unix_ms": self.expected_period_start_unix_ms,
            "expected_period_end_unix_ms": self.expected_period_end_unix_ms,
            "invoice_period_start_unix_ms": self.invoice_period_start_unix_ms,
            "invoice_period_end_unix_ms": self.invoice_period_end_unix_ms,
            "expected_model": self.expected_model,
            "invoice_model": self.invoice_model,
            "expected_usage_digest": self.expected_usage_digest,
            "invoice_usage_digest": self.invoice_usage_digest,
            "expected_amount": self.expected_amount,
            "invoice_amount": self.invoice_amount,
            "state": self.state,
            "discrepancy_codes": self.discrepancy_codes,
            "correction_ref": self.correction_ref,
        }))
    }
}

pub fn validate_invoice_comparison(comparison: &InvoiceComparison) -> Result<(), &'static str> {
    comparison.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\n', '\r'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn has_duplicates(values: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value))
}
