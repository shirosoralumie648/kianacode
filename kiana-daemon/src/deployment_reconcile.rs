//! DaemonHost route for the pure reconcile decision reducer.

use kiana_domain::{ReconcileInput, ReconcileReport};
use kiana_ports::PortError;

pub(crate) fn evaluate(input: &ReconcileInput) -> Result<ReconcileReport, PortError> {
    kiana_core::evaluate_reconcile(input).map_err(PortError::Failed)
}
