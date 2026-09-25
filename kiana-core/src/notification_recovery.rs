//! Core facade for the pure notification recovery planner.
//!
//! This module keeps cancellation/revocation/expiry/Unknown decisions on the same server-owned
//! contract without dispatching a connector or turning reconciliation into an automatic retry.

pub use kiana_domain::{
    NotificationRecoveryDisposition, NotificationRecoveryInput, NotificationRecoveryPlan,
    NotificationRecoveryState, NOTIFICATION_RECOVERY_SCHEMA,
};

pub fn plan_notification_recovery(
    input: &NotificationRecoveryInput,
) -> Result<NotificationRecoveryPlan, String> {
    input.plan()
}
