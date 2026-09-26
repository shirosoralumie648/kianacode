//! Core adapter for the typed Company closing receipt.
//!
//! Closing remains a projection/validation boundary here; CompanyState/EventLog commit the
//! underlying transition and no receipt is inferred from a chat or a single runtime result.

use kiana_domain::{CompanyClosingReceiptContract, CompanyClosingReceiptLedger};

pub(crate) fn record_closing_receipt(
    ledger: &mut CompanyClosingReceiptLedger,
    receipt: CompanyClosingReceiptContract,
) -> Result<(), &'static str> {
    ledger.record(receipt)
}
