//! BQ-20 committed source-page adapter.
//!
//! This module only reads the existing `EventStorePort`. It does not create a second ledger or
//! append quarantine/projection facts. A caller persists the query projection separately and must
//! re-read from the last acknowledged cursor after restart.

use kiana_domain::{EventCursor, JournalPage, RuntimeEvent};
use kiana_ports::{EventStorePort, PortError};

#[derive(Clone, Debug, PartialEq)]
pub struct BillingSourcePage {
    pub source_epoch: u64,
    pub after_cursor: EventCursor,
    pub first_cursor: Option<EventCursor>,
    pub page: JournalPage,
}

impl BillingSourcePage {
    pub fn new(
        source_epoch: u64,
        after_cursor: EventCursor,
        page: JournalPage,
    ) -> Result<Self, PortError> {
        if source_epoch == 0 {
            return Err(PortError::Failed("billing_source_epoch_invalid".to_owned()));
        }
        if page.cursor < after_cursor {
            return Err(PortError::Conflict(
                "billing_source_cursor_regressed".to_owned(),
            ));
        }
        let expected_count = page.cursor - after_cursor;
        if expected_count != page.events.len() as u64 {
            return Err(PortError::Conflict(
                "billing_source_cursor_not_contiguous".to_owned(),
            ));
        }
        let first_cursor =
            if expected_count > 0 {
                Some(after_cursor.checked_add(1).ok_or_else(|| {
                    PortError::Failed("billing_source_cursor_overflow".to_owned())
                })?)
            } else {
                None
            };
        Ok(Self {
            source_epoch,
            after_cursor,
            first_cursor,
            page,
        })
    }

    pub fn cursor(&self) -> EventCursor {
        self.page.cursor
    }

    pub fn events(&self) -> &[RuntimeEvent] {
        &self.page.events
    }
}

/// Read one complete committed page. `read_from` never returns a partial transition, so the
/// returned page can be handed to the projector without fabricating per-request sequence cursors.
pub async fn read_billing_page(
    store: &dyn EventStorePort,
    source_epoch: u64,
    after_cursor: EventCursor,
    limit: usize,
) -> Result<BillingSourcePage, PortError> {
    let page = store.read_from(after_cursor, limit).await?;
    BillingSourcePage::new(source_epoch, after_cursor, page)
}

/// Explicit alias for adapters that use “ledger” terminology in their source loop.
pub async fn read_ledger_page(
    store: &dyn EventStorePort,
    source_epoch: u64,
    after_cursor: EventCursor,
    limit: usize,
) -> Result<BillingSourcePage, PortError> {
    read_billing_page(store, source_epoch, after_cursor, limit).await
}

pub const BILLING_SOURCE_IS_READ_ONLY: bool = true;
