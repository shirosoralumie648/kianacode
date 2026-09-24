//! Compatibility entry point for the BQ-14 correction contract.
//!
//! The immutable ledger and correction reducer live together in `billing_ledger` so the source
//! of truth cannot split into a second ledger.  This module keeps the roadmap-facing file name
//! available to adapters and source guards without defining another authority.

pub use crate::billing_ledger::{
    CostCorrection, CostCorrectionAppendOutcome, CostCorrectionApproval, CostCorrectionCommand,
    COST_CORRECTION_APPROVAL_SCHEMA, COST_CORRECTION_COMMAND, COST_CORRECTION_COMMAND_SCHEMA,
    COST_CORRECTION_EVENT, COST_CORRECTION_SCHEMA, MAX_COST_CORRECTION_EVIDENCE,
    MAX_COST_CORRECTION_REASON,
};
