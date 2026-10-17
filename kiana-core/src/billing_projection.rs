//! BQ-20 ControlPlane-side projection fence.
//!
//! Folding lives in the query crate, while this small core contract owns the authority boundary:
//! a candidate projection must be written/validated before its source cursor is acknowledged.
//! It never appends a fact and it never reads reservations or approvals.

use kiana_domain::{
    billing_replay_fence, billing_source_event_id_tail, BillingProjectionCursor,
    BillingProjectionSnapshot, EventCursor, JournalPage, BILLING_PROJECTION_NUMBER,
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
    #[error("billing_projection_source_fence_mismatch")]
    SourceFenceMismatch,
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
    ///
    /// The candidate is not trusted just because its snapshot is internally valid: its replay
    /// fence must chain exactly from this fence over the committed page events, and its bounded
    /// source-event tail must equal the previous tail extended by this page. A candidate carried
    /// over from an older page (stale) or assembled from a different event order therefore fails
    /// (`SourceFenceMismatch`) and the acknowledged cursor never moves.
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
        // Recompute the expected replay fence and bounded event-ID tail from this fence plus the
        // committed page. The acknowledgement must cover exactly this page, in this order.
        let mut expected_fence = self.cursor.replay_fence.clone();
        let mut cursor = self.cursor.source_cursor;
        let mut page_event_ids = Vec::with_capacity(page.events.len());
        for event in &page.events {
            cursor = cursor
                .checked_add(1)
                .ok_or(BillingProjectionFenceError::CursorGap)?;
            expected_fence = billing_replay_fence(&expected_fence, cursor, event.event_id);
            page_event_ids.push(event.event_id);
        }
        let expected_ids =
            billing_source_event_id_tail(&self.cursor.source_event_ids, &page_event_ids);
        if candidate.source.replay_fence != expected_fence
            || candidate.source.source_event_ids != expected_ids
        {
            return Err(BillingProjectionFenceError::SourceFenceMismatch);
        }
        self.cursor = candidate.source.clone();
        Ok(())
    }

    pub fn source_cursor(&self) -> EventCursor {
        self.cursor.source_cursor
    }
}

pub const BILLING_PROJECTION_FENCE_NO_FACT_WRITES: bool = true;
