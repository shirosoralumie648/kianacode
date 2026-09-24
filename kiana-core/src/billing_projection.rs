//! BQ-20 ControlPlane-side projection fence.
//!
//! Folding lives in the query crate, while this small core contract owns the authority boundary:
//! a candidate projection must be written/validated before its source cursor is acknowledged.
//! It never appends a fact and it never reads reservations or approvals.

use kiana_domain::{
    BillingProjectionCursor, BillingProjectionSnapshot, EventCursor, JournalPage,
    BILLING_PROJECTION_NUMBER,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BillingProjectionFenceError {
    #[error("billing_projection_epoch_invalid")]
    EpochInvalid,
    #[error("billing_projection_cursor_regressed")]
    CursorRegressed,
    #[error("billing_projection_cursor_gap")]
    CursorGap,
    #[error("billing_projection_version_mismatch")]
    VersionMismatch,
    #[error("billing_projection_candidate_invalid:{0}")]
    CandidateInvalid(String),
}

/// Server-owned acknowledgement fence for a read-model cursor. The fence is not a ledger and is
/// safe to discard; the EventLog remains the replay source.
#[derive(Clone, Debug, PartialEq)]
pub struct BillingProjectionFence {
    cursor: BillingProjectionCursor,
}

impl BillingProjectionFence {
    pub fn new(source_epoch: u64) -> Result<Self, BillingProjectionFenceError> {
        if source_epoch == 0 {
            return Err(BillingProjectionFenceError::EpochInvalid);
        }
        let cursor = BillingProjectionCursor::initial(source_epoch)
            .map_err(BillingProjectionFenceError::CandidateInvalid)?;
        Ok(Self { cursor })
    }

    pub fn cursor(&self) -> &BillingProjectionCursor {
        &self.cursor
    }

    /// Validate a candidate assembled from one committed page. Only after this returns `Ok` may a
    /// caller persist/acknowledge the candidate cursor. A failed candidate leaves the fence intact.
    pub fn stage(
        &mut self,
        page: &JournalPage,
        candidate: &BillingProjectionSnapshot,
        source_epoch: u64,
    ) -> Result<(), BillingProjectionFenceError> {
        if source_epoch == 0 || source_epoch != self.cursor.source_epoch {
            return Err(BillingProjectionFenceError::EpochInvalid);
        }
        candidate
            .validate()
            .map_err(BillingProjectionFenceError::CandidateInvalid)?;
        if candidate.source.source_epoch != source_epoch
            || candidate.projection_version != BILLING_PROJECTION_NUMBER
        {
            return Err(BillingProjectionFenceError::VersionMismatch);
        }
        let expected = self
            .cursor
            .source_cursor
            .checked_add(page.events.len() as u64)
            .ok_or(BillingProjectionFenceError::CursorGap)?;
        if page.cursor < self.cursor.source_cursor {
            return Err(BillingProjectionFenceError::CursorRegressed);
        }
        if page.cursor != expected || candidate.source.source_cursor != page.cursor {
            return Err(BillingProjectionFenceError::CursorGap);
        }
        self.cursor = candidate.source.clone();
        Ok(())
    }

    pub fn source_cursor(&self) -> EventCursor {
        self.cursor.source_cursor
    }
}

pub const BILLING_PROJECTION_FENCE_NO_FACT_WRITES: bool = true;
