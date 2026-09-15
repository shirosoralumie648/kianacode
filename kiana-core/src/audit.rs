//! Control-plane facade for the pure committed-event audit reducer.
//!
//! Keeping this wrapper in core makes the ownership boundary explicit: adapters may read a
//! committed EventLog page and ask core for a projection, but no audit row is accepted from a
//! model, UI, plugin, exporter or capability handler.

use kiana_domain::{AuditRecord, EventCursor, RuntimeEvent};

/// Derive append-only audit records from an already committed EventLog slice.
pub fn reduce_committed_audit_records(
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
) -> Result<Vec<AuditRecord>, String> {
    kiana_domain::reduce_audit_records(events, source_cursor)
}

/// Append a newly reduced committed page while preserving all existing audit facts.
pub fn append_committed_audit_records(
    existing: &[AuditRecord],
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
) -> Result<Vec<AuditRecord>, String> {
    kiana_domain::append_audit_records(existing, events, source_cursor)
}
