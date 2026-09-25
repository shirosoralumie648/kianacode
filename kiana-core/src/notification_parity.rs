//! Core facade for cross-entry notification parity comparison.

pub use kiana_domain::{
    NotificationEntrypoint, NotificationEntrypointParity, NotificationEntrypointSnapshot,
    NotificationParityDisposition, NOTIFICATION_ENTRYPOINT_PARITY_SCHEMA,
    NOTIFICATION_ENTRYPOINT_SNAPSHOT_SCHEMA,
};

pub fn compare_notification_entrypoints(
    snapshots: Vec<NotificationEntrypointSnapshot>,
) -> Result<NotificationEntrypointParity, String> {
    NotificationEntrypointParity::compare(snapshots)
}
