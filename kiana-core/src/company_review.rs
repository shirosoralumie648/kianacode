//! Core adapter for the pure independent Company review contract.

use kiana_domain::{IndependentReview, IndependentReviewLedger};

pub(crate) fn record_independent_review(
    ledger: &mut IndependentReviewLedger,
    review: IndependentReview,
) -> Result<(), &'static str> {
    ledger.record(review)
}
