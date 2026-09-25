//! Core facade for the replay-only notification fault matrix.

pub use kiana_domain::{
    NotificationFaultCase, NotificationFaultDisposition, NotificationFaultMatrix,
    NotificationFaultScenario, NOTIFICATION_FAULT_CASE_SCHEMA, NOTIFICATION_FAULT_MATRIX_SCHEMA,
};

pub fn notification_fault_matrix(
    seed: u64,
    source_cursor: u64,
    source_event_ids: Vec<String>,
) -> Result<NotificationFaultMatrix, String> {
    NotificationFaultMatrix::new(seed, source_cursor, source_event_ids)
}
